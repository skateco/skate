mod filter;
mod least_pods;
mod node_name;
mod node_resources_fit;
mod plugins;
mod pod_scheduler;
mod priority_sort;
mod resource_allocation;
mod unschedulable;

use crate::node_client::NodeClients;
use crate::scheduler::pod_scheduler::PodScheduler;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::PodmanPodStatus;
use crate::spec::cert::ClusterIssuer;
use crate::state::state::{CatalogueItem, ClusterState, NodeState};
use crate::supported_resources::SupportedResources;
use crate::util::{hash_k8s_resource, metadata_name, NamespacedName, SkateLabels, CROSS_EMOJI};
use anyhow::anyhow;
use async_trait::async_trait;
use colored::Colorize;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, RollingUpdateDeployment};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Pod, Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::Metadata;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::error::Error;

#[derive(Debug)]
pub struct ScheduleResult {
    pub placements: Vec<ScheduledOperation>,
}

#[async_trait(? Send)]
pub trait Scheduler {
    async fn schedule(
        &self,
        conns: &NodeClients,
        state: &mut ClusterState,
        objects: Vec<SupportedResources>,
        dry_run: bool,
    ) -> Result<ScheduleResult, Box<dyn Error>>;
}

pub struct DefaultScheduler {
    pod_scheduler: PodScheduler,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpType {
    Info,
    Create,
    Clobber,
    Delete,
    Unchanged,
}

impl OpType {
    pub fn symbol(&self) -> String {
        match self {
            OpType::Clobber => "~".green().bold(),
            OpType::Info => "[i]".blue().bold(),
            OpType::Create => "+".green().bold(),
            OpType::Delete => "-".red().bold(),
            OpType::Unchanged => "".blue().bold(),
        }
        .to_string()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct ScheduledOperation {
    pub resource: SupportedResources,
    pub node: Option<NodeState>,
    pub operation: OpType,
    pub error: Option<String>,
    pub silent: bool,
}

impl ScheduledOperation {
    pub fn new(op: OpType, resource: SupportedResources) -> Self {
        ScheduledOperation {
            resource,
            node: None,
            operation: op,
            error: None,
            silent: false,
        }
    }
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }
    pub fn node(mut self, n: NodeState) -> Self {
        self.node = Some(n);
        self
    }
    pub fn error(mut self, err: String) -> Self {
        self.error = Some(err);
        self
    }
}

#[derive(Debug, PartialEq)]
pub struct ApplyPlan {
    pub actions: HashMap<NamespacedName, Vec<ScheduledOperation>>,
}

#[derive(Debug)]
pub struct RejectedNode {
    pub node_name: String,
    pub reason: String,
}

pub struct NodeSelection {
    pub selected: Option<NodeState>,
    pub rejected: Vec<RejectedNode>,
}

// 3 types of planning:
// 1 per node (service, ingress, secret)
// maybe > 0 per node (daemonset)
// distributed (pod, cron)
impl DefaultScheduler {
    pub fn new() -> Self {
        DefaultScheduler {
            pod_scheduler: PodScheduler::new(),
        }
    }
    fn next_generation(existing: Option<&CatalogueItem>) -> i64 {
        if let Some(existing_gen) = existing.and_then(|m| Some(m.object.generation.clone())) {
            if existing_gen > 0 {
                return existing_gen + 1;
            } else {
                return 1;
            }
        };
        1
    }

    fn plan_daemonset(state: &ClusterState, ds: &DaemonSet) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut ds = ds.clone();
        let daemonset_name = ds
            .metadata
            .name
            .clone()
            .ok_or(anyhow!("no daemonset name"))?;

        let ns = ds.metadata.namespace.clone().unwrap_or("".to_string());
        let existing_daemonset = state.catalogue(
            None,
            &[ResourceType::DaemonSet],
            Some(&ns),
            Some(&daemonset_name),
        );
        ds.metadata.generation = Some(Self::next_generation(existing_daemonset.first()));

        let default_ops: Vec<_> = state
            .nodes
            .iter()
            .map(|n| {
                ScheduledOperation::new(OpType::Create, SupportedResources::DaemonSet(ds.clone()))
                    .silent()
                    .node(n.clone())
            })
            .collect();

        let mut actions = HashMap::from([(metadata_name(&ds), default_ops)]);

        let schedulable_nodes = state.nodes.iter().filter(|n| n.schedulable());
        let unschedulable_nodes = state.nodes.iter().filter(|n| !n.schedulable());

        for node in unschedulable_nodes {
            let existing_pods = node.filter_pods(&|p| {
                p.labels.contains_key(&SkateLabels::Daemonset.to_string())
                    && p.labels.get(&SkateLabels::Daemonset.to_string()).unwrap() == &daemonset_name
                    && p.labels.get(&SkateLabels::Namespace.to_string()).unwrap() == &ns
            });
            for pod_info in existing_pods {
                let pod: Pod = (&pod_info).into();
                let name =
                    NamespacedName::from(pod.metadata.name.clone().unwrap_or_default().as_str());
                let op = ScheduledOperation::new(OpType::Delete, SupportedResources::Pod(pod))
                    .node(node.clone());
                match actions.get_mut(&name) {
                    Some(ops) => ops.push(op),
                    None => {
                        actions.insert(name, vec![op]);
                    }
                };
            }
        }

        for node in schedulable_nodes {
            let node_name = node.node_name.clone();
            let mut pod_spec = ds
                .spec
                .clone()
                .map(|s| s.template)
                .and_then(|t| t.spec)
                .unwrap_or_default();

            // inherit daemonset labels
            let mut meta = ds
                .spec
                .as_ref()
                .and_then(|s| s.template.metadata.clone())
                .unwrap_or_default();

            let name = format!("dms-{}-{}", daemonset_name.clone(), node_name);

            let ns_name = NamespacedName {
                name: name.clone(),
                namespace: ns.clone(),
            };

            meta.name = Some(ns_name.to_string());
            meta.namespace = Some(ns.clone());

            let mut labels = meta.labels.clone().unwrap_or_default();
            labels.insert(SkateLabels::Name.to_string(), name.clone());
            labels.insert(SkateLabels::Daemonset.to_string(), daemonset_name.clone());
            meta.labels = Some(labels);

            // bind to specific node
            pod_spec.node_selector = Some({
                let mut selector = pod_spec.node_selector.unwrap_or_default();
                selector.insert(SkateLabels::Nodename.to_string(), node_name.clone());
                selector
            });

            let pod = Pod {
                metadata: meta,
                spec: Some(pod_spec),
                status: None,
            };

            let result = Self::plan_pod(state, &pod)?;
            actions.insert(ns_name, result);
        }

        Ok(ApplyPlan { actions })
    }

    fn plan_deployment(state: &ClusterState, d: &Deployment) -> Result<ApplyPlan, Box<dyn Error>> {
        // check if  there are more pods than replicas running
        // cull them if so
        let strategy = d
            .spec
            .clone()
            .unwrap_or_default()
            .strategy
            .unwrap_or_default();

        let is_rolling = match strategy.type_.clone().unwrap_or_default().as_str() {
            "RollingUpdate" => true,
            "" | "Recreate" => false,
            _ => {
                return Err(anyhow!(
                    "unrecognised strategy {}",
                    strategy.type_.unwrap_or_default()
                )
                .into())
            }
        };

        if is_rolling {
            return Self::plan_deployment_rolling_update(
                state,
                d,
                strategy.rolling_update.unwrap_or_default(),
            );
        }
        Self::plan_deployment_recreate(state, d)
    }

    fn plan_deployment_rolling_update(
        state: &ClusterState,
        d: &Deployment,
        _: RollingUpdateDeployment,
    ) -> Result<ApplyPlan, Box<dyn Error>> {
        let actions = Self::plan_deployment_generic(state, d)?;

        // max unavailable = 25% == 75% must be available
        // max surge = 25% == max 125% number of pods
        // deploy
        // deploy to max surge
        // delete to max unavail
        // deploy to max surge
        // etc

        // TODO - respect max surge and max unavailable
        // will require parallelism.
        // And an OpType::Parallel

        Ok(actions)

        //
    }

    fn plan_deployment_recreate(
        state: &ClusterState,
        d: &Deployment,
    ) -> Result<ApplyPlan, Box<dyn Error>> {
        let plan = Self::plan_deployment_generic(state, d)?;

        // have one top level key, with all the actions, sorted by delete, create, unchanged

        let mut new_actions = vec![];

        for (_, v) in plan.actions {
            new_actions.extend(v)
        }

        new_actions = new_actions
            .into_iter()
            .sorted_by(|a, _| match a.operation {
                OpType::Delete => Ordering::Less,
                OpType::Create => Ordering::Greater,
                _ => Ordering::Equal,
            })
            .collect();

        Ok(ApplyPlan {
            actions: HashMap::from([(metadata_name(d), new_actions)]),
        })
    }

    fn plan_deployment_generic(
        state: &ClusterState,
        d: &Deployment,
    ) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut d = d.clone();

        let deployment_name = d.metadata.name.clone().unwrap_or("".to_string());
        let ns = d.metadata.namespace.clone().unwrap_or("".to_string());

        let existing_deployment = state.catalogue(
            None,
            &[ResourceType::Deployment],
            Some(&ns),
            Some(&deployment_name),
        );
        d.metadata.generation = Some(Self::next_generation(existing_deployment.first()));

        let replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);

        if deployment_name.is_empty() {
            return Err(anyhow!("no deployment name").into());
        }

        let default_ops: Vec<_> = state
            .nodes
            .iter()
            .map(|n| {
                ScheduledOperation::new(OpType::Create, SupportedResources::Deployment(d.clone()))
                    .silent()
                    .node(n.clone())
            })
            .collect();

        let mut actions: HashMap<_, Vec<_>> = HashMap::from([(metadata_name(&d), default_ops)]);

        // regardless what happens, overwrite the deployment manifest to reflect the current one

        let existing_pods = state.locate_deployment_pods(&deployment_name, &ns);

        let existing_pods: Vec<_> = existing_pods
            .into_iter()
            .map(|(dp, node)| {
                let replica = dp
                    .labels
                    .get(&SkateLabels::Replica.to_string())
                    .unwrap_or(&"0".to_string())
                    .clone();
                let replica = replica.parse::<u32>().unwrap_or(0);
                (dp, node, replica)
            })
            .sorted_by_key(|(_, _, replica)| *replica)
            .rev()
            .collect();

        if existing_pods.len() > replicas as usize {
            // cull the extra pods
            for (pod_info, node, replica) in existing_pods {
                if replica >= replicas as u32 {
                    let pod: Pod = (&pod_info).into();
                    let name = NamespacedName::from(
                        pod.metadata.name.clone().unwrap_or_default().as_str(),
                    );
                    let op = ScheduledOperation::new(OpType::Delete, SupportedResources::Pod(pod))
                        .node(node.clone());
                    match actions.get_mut(&name) {
                        Some(ops) => ops.push(op),
                        None => {
                            actions.insert(name, vec![op]);
                        }
                    };
                }
            }
        }

        for i in 0..replicas {
            let pod_spec = d
                .spec
                .clone()
                .map(|s| s.template)
                .and_then(|t| t.spec)
                .unwrap_or_default();

            // inherit deployment labels
            let mut meta = d
                .spec
                .as_ref()
                .and_then(|s| s.template.metadata.clone())
                .unwrap_or_default();
            // name format needs to be <type>.<fqn>.<replica>
            let name = format!("dpl-{}-{}", deployment_name, i);
            let ns_name = NamespacedName {
                name: name.clone(),
                namespace: ns.clone(),
            };
            // needs to be the fqn for kube play, since that's what it'll call the pod
            meta.name = Some(ns_name.to_string());
            meta.namespace = Some(ns.clone());

            let mut labels = meta.labels.unwrap_or_default();
            labels.insert(SkateLabels::Name.to_string(), name);
            labels.insert(SkateLabels::Deployment.to_string(), deployment_name.clone());
            labels.insert(SkateLabels::Replica.to_string(), i.to_string());
            meta.labels = Some(labels);

            let pod = Pod {
                metadata: meta,
                spec: Some(pod_spec),
                status: None,
            };

            let result = Self::plan_pod(state, &pod)?;

            match actions.get_mut(&ns_name) {
                Some(ops) => ops.extend(result),
                None => {
                    actions.insert(ns_name, result);
                }
            };
        }

        Ok(ApplyPlan { actions })
    }

    fn plan_pod(
        state: &ClusterState,
        object: &Pod,
    ) -> Result<Vec<ScheduledOperation>, Box<dyn Error>> {
        let mut new_pod = object.clone();

        let new_hash = hash_k8s_resource(&mut new_pod);

        let name = metadata_name(object);

        // smuggle node selectors as labels
        if let Some(spec) = new_pod.spec.as_ref() {
            if spec.node_selector.is_some() {
                let ns = spec.node_selector.clone().unwrap();
                let selector_labels = ns
                    .iter()
                    .map(|(k, v)| (format!("nodeselector/{}", k), v.clone()));
                let mut labels = new_pod.metadata().labels.clone().unwrap_or_default();
                labels.extend(selector_labels);
                new_pod.metadata_mut().labels = Some(labels)
            }
        }

        let existing_pods = state.locate_pods(&name.name, &name.namespace);

        // existing pods with same name (duplicates if more than 1)
        // sort by replicas descending
        let cull_actions: Vec<_> = match existing_pods.len() {
            0 | 1 => vec![], // none or 1 already, that's ok
            _ => existing_pods.as_slice()[1..]
                .iter()
                .map(|(pod_info, node)| {
                    ScheduledOperation::new(
                        OpType::Delete,
                        SupportedResources::Pod(pod_info.into()),
                    )
                    .node((**node).clone())
                })
                .collect(),
        };

        let existing_pod = &existing_pods.first();

        let op_types = match existing_pod {
            Some((pod_info, node)) => {
                let previous_hash = pod_info
                    .labels
                    .get(&SkateLabels::Hash.to_string())
                    .unwrap_or(&"".to_string())
                    .clone();
                let state_running = pod_info.status == PodmanPodStatus::Running;

                let hash_matches = previous_hash.clone() == new_hash;
                match hash_matches && state_running && node.schedulable() {
                    true => vec![(OpType::Unchanged, Some((**node).clone()))],
                    false => vec![
                        (OpType::Delete, Some((**node).clone())),
                        (OpType::Create, None),
                    ],
                }
            }
            None => vec![(OpType::Create, None)],
        };

        let actions = op_types
            .into_iter()
            .map(|(op, node)| ScheduledOperation {
                resource: SupportedResources::Pod(new_pod.clone()),
                node,
                operation: op,
                error: None,
                silent: false,
            })
            .collect();

        Ok([cull_actions, actions].concat())
    }

    fn plan_cronjob(state: &ClusterState, cron: &CronJob) -> Result<ApplyPlan, Box<dyn Error>> {
        let name = metadata_name(cron);

        let mut new_cron = cron.clone();

        let existing_cron = state.catalogue(
            None,
            &[ResourceType::CronJob],
            Some(&name.namespace),
            Some(&name.name),
        );
        let existing_cron = existing_cron.first();
        new_cron.metadata.generation = Some(Self::next_generation(existing_cron));
        // Sanitise manifest since we'll be running that later via kube play
        // - only 1 replica
        let mut actions = vec![];

        let new_hash = hash_k8s_resource(&mut new_cron);

        let op_types = match existing_cron {
            Some(existing_cron) => {
                let node = existing_cron.nodes.first().unwrap();
                if existing_cron.object.manifest_hash == new_hash && node.schedulable() {
                    vec![(OpType::Unchanged, Some(node))]
                } else {
                    vec![(OpType::Delete, Some(node)), (OpType::Create, None)]
                }
            }
            None => {
                vec![(OpType::Create, None)]
            }
        };
        op_types.into_iter().for_each(|(op_type, node)| {
            actions.push(ScheduledOperation {
                operation: op_type,
                resource: SupportedResources::CronJob(new_cron.clone()),
                node: node.cloned().cloned(),
                error: None,
                silent: false,
            })
        });

        // check if we have an existing cronjob for this
        // if so compare hashes, if differ then create, otherwise no change

        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    fn plan_secret(state: &ClusterState, secret: &Secret) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut new_secret = secret.clone();

        let mut actions = vec![];
        let ns_name = metadata_name(&new_secret);

        let existing_secret = state.catalogue(
            None,
            &[ResourceType::Secret],
            Some(&ns_name.namespace),
            Some(&ns_name.name),
        );
        new_secret.metadata.generation = Some(Self::next_generation(existing_secret.first()));

        let new_hash = hash_k8s_resource(&mut new_secret);

        let mut op_types: Vec<_> = vec![];

        for node in state.nodes.iter() {
            let existing_secrets = state.catalogue(
                Some(&node.node_name),
                &[ResourceType::Secret],
                Some(&ns_name.namespace),
                Some(&ns_name.name),
            );
            let existing_secret = existing_secrets.first();

            op_types.extend(match existing_secret {
                Some(existing_secret) => {
                    if !node.schedulable() {
                        vec![(OpType::Delete, node)]
                    } else if existing_secret.object.manifest_hash == new_hash {
                        vec![(OpType::Unchanged, node)]
                    } else {
                        vec![(OpType::Clobber, node)]
                    }
                }
                None => {
                    if node.schedulable() {
                        vec![(OpType::Create, node)]
                    } else {
                        vec![]
                    }
                }
            });
        }

        for (op, node) in op_types {
            actions.push(
                ScheduledOperation::new(op, SupportedResources::Secret(new_secret.clone()))
                    .node(node.clone()),
            );
        }

        Ok(ApplyPlan {
            actions: HashMap::from([(ns_name, actions)]),
        })
    }
    fn plan_service(state: &ClusterState, service: &Service) -> Result<ApplyPlan, Box<dyn Error>> {
        let name = metadata_name(service);

        if let Some(spec) = &service.spec {
            if let Some(selector) = &spec.selector {
                if selector.is_empty() {
                    return Err(anyhow!("service selector can not be empty").into());
                }
            }
        }

        let mut actions = vec![];

        let mut new_service = service.clone();

        let existing_service = state.catalogue(
            None,
            &[ResourceType::Service],
            Some(&name.namespace),
            Some(&name.name),
        );
        new_service.metadata.generation = Some(Self::next_generation(existing_service.first()));

        let new_hash = hash_k8s_resource(&mut new_service);

        for node in state.nodes.iter() {
            let existing_service = state.catalogue(
                Some(&node.node_name),
                &[ResourceType::Service],
                Some(&name.namespace),
                Some(&name.name),
            );

            let existing_service = existing_service.first();

            let op_types = match existing_service {
                Some(existing_service) => {
                    if !node.schedulable() {
                        vec![OpType::Delete]
                    } else if existing_service.object.manifest_hash == new_hash {
                        vec![OpType::Unchanged]
                    } else {
                        vec![OpType::Delete, OpType::Create]
                    }
                }
                None => {
                    if node.schedulable() {
                        vec![OpType::Create]
                    } else {
                        vec![]
                    }
                }
            };
            op_types.into_iter().for_each(|op_type| {
                actions.push(
                    ScheduledOperation::new(
                        op_type,
                        SupportedResources::Service(new_service.clone()),
                    )
                    .node(node.clone()),
                )
            });
        }

        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    fn plan_ingress(state: &ClusterState, ingress: &Ingress) -> Result<ApplyPlan, Box<dyn Error>> {
        // TODO - warn about unsupported settings
        let mut actions = vec![];

        let mut new_ingress = ingress.clone();

        let new_hash = hash_k8s_resource(&mut new_ingress);

        let name = metadata_name(ingress);

        let existing_ingress = state.catalogue(
            None,
            &[ResourceType::Ingress],
            Some(&name.namespace),
            Some(&name.name),
        );

        new_ingress.metadata.generation = Some(Self::next_generation(existing_ingress.first()));

        for node in state.nodes.iter() {
            let existing_ingress = state.catalogue(
                Some(&node.node_name),
                &[ResourceType::Ingress],
                Some(&name.namespace),
                Some(&name.name),
            );
            let existing_ingress = existing_ingress.first();

            let op_types = match existing_ingress {
                Some(existing_ingress) => {
                    if !node.schedulable() {
                        vec![OpType::Delete]
                    } else if existing_ingress.object.manifest_hash == new_hash {
                        vec![OpType::Unchanged]
                    } else {
                        vec![OpType::Delete, OpType::Create]
                    }
                }
                None => {
                    if node.schedulable() {
                        vec![OpType::Create]
                    } else {
                        vec![]
                    }
                }
            };
            op_types.into_iter().for_each(|op_type| {
                actions.push(
                    ScheduledOperation::new(
                        op_type,
                        SupportedResources::Ingress(new_ingress.clone()),
                    )
                    .node(node.clone()),
                )
            });
        }

        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    fn plan_cluster_issuer(
        state: &mut ClusterState,
        cluster_issuer: &ClusterIssuer,
    ) -> Result<ApplyPlan, Box<dyn Error>> {
        let ns_name = metadata_name(cluster_issuer);

        let mut actions = vec![];

        let mut new_cluster_issuer = cluster_issuer.clone();

        let existing_cluster_issuer = state.catalogue(
            None,
            &[ResourceType::ClusterIssuer],
            Some("skate"),
            Some(&ns_name.name),
        );

        new_cluster_issuer.metadata.generation =
            Some(Self::next_generation(existing_cluster_issuer.first()));

        let new_hash = hash_k8s_resource(&mut new_cluster_issuer);

        for node in state.nodes.iter() {
            let existing = state.catalogue(
                Some(&node.node_name),
                &[ResourceType::ClusterIssuer],
                Some("skate"),
                Some(&ns_name.name),
            );

            let op_types = match existing.first() {
                Some(existing) => {
                    if !node.schedulable() {
                        vec![OpType::Delete]
                    } else if existing.object.manifest_hash == new_hash {
                        vec![OpType::Unchanged]
                    } else {
                        vec![OpType::Delete, OpType::Create]
                    }
                }
                None => {
                    if node.schedulable() {
                        vec![OpType::Create]
                    } else {
                        vec![]
                    }
                }
            };
            op_types.into_iter().for_each(|op_type| {
                actions.push(
                    ScheduledOperation::new(
                        op_type,
                        SupportedResources::ClusterIssuer(new_cluster_issuer.clone()),
                    )
                    .node(node.clone()),
                )
            });
        }

        Ok(ApplyPlan {
            actions: HashMap::from([(ns_name, actions)]),
        })
    }
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(
        state: &mut ClusterState,
        object: &SupportedResources,
    ) -> Result<ApplyPlan, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => {
                let ns_name = NamespacedName {
                    name: pod.metadata.name.clone().unwrap_or_default(),
                    namespace: pod.metadata.namespace.clone().unwrap_or_default(),
                };
                let ops = Self::plan_pod(state, pod)?;
                Ok(ApplyPlan {
                    actions: HashMap::from([(ns_name, ops)]),
                })
            }
            SupportedResources::Deployment(deployment) => Self::plan_deployment(state, deployment),
            SupportedResources::DaemonSet(ds) => Self::plan_daemonset(state, ds),
            SupportedResources::Ingress(ingress) => Self::plan_ingress(state, ingress),
            SupportedResources::CronJob(cron) => Self::plan_cronjob(state, cron),
            SupportedResources::Secret(secret) => Self::plan_secret(state, secret),
            SupportedResources::Service(service) => Self::plan_service(state, service),
            SupportedResources::ClusterIssuer(issuer) => Self::plan_cluster_issuer(state, issuer),
        }
    }

    async fn remove_existing(
        conns: &NodeClients,
        resource: ScheduledOperation,
    ) -> Result<(String, String), Box<dyn Error>> {
        let hook_result = resource
            .resource
            .pre_remove_hook(resource.node.as_ref().unwrap(), conns)
            .await;

        let conn = conns
            .find(&resource.node.unwrap().node_name)
            .ok_or("failed to find connection to host")?;

        let manifest =
            serde_yaml::to_string(&resource.resource).expect("failed to serialize manifest");
        let remove_result = conn.remove_resource_by_manifest(&manifest).await;

        if hook_result.is_err() {
            return Err(hook_result.err().unwrap());
        }

        remove_result
    }

    async fn apply(
        &self,
        plan: ApplyPlan,
        conns: &NodeClients,
        state: &mut ClusterState,
        dry_run: bool,
    ) -> Result<Vec<ScheduledOperation>, Box<dyn Error>> {
        let mut result: Vec<ScheduledOperation> = vec![];

        for (_name, ops) in plan.actions {
            for mut op in ops {
                match op.operation {
                    OpType::Delete => {
                        let node_name = op.node.clone().unwrap().node_name;
                        if dry_run {
                            let _ = state.reconcile_object_deletion(&op.resource, &node_name)?;
                            if !op.silent {
                                println!(
                                    "{} {} {} deleted on node {} ",
                                    op.operation.symbol(),
                                    op.resource,
                                    op.resource.name(),
                                    node_name
                                );
                            }
                            continue;
                        }

                        match Self::remove_existing(conns, op.clone()).await {
                            Ok((_, stderr)) => {
                                // println!("{}", stdout.trim());
                                if !stderr.is_empty() {
                                    eprintln!("{}", stderr.trim())
                                }

                                let _ =
                                    state.reconcile_object_deletion(&op.resource, &node_name)?;
                                if !op.silent {
                                    println!(
                                        "{} {} {} deleted on node {} ",
                                        op.operation.symbol(),
                                        op.resource,
                                        op.resource.name(),
                                        node_name
                                    );
                                }
                                result.push(op.clone());
                            }
                            Err(err) => {
                                op.error = Some(err.to_string());
                                println!(
                                    "{} failed to delete {} on node {}: {}",
                                    CROSS_EMOJI,
                                    op.resource.name(),
                                    node_name,
                                    err
                                );
                                result.push(op.clone());
                            }
                        }
                    }
                    OpType::Create | OpType::Clobber => {
                        let selection = match op.node.clone() {
                            // some things like ingress have the node already set
                            Some(n) => NodeSelection {
                                selected: Some(n),
                                rejected: vec![],
                            },
                            // anything else and things with node selectors go here
                            None => {
                                match op.resource {
                                    SupportedResources::Pod(ref pod) => {
                                        self.pod_scheduler.choose_node(&state.nodes, pod)
                                    }
                                    SupportedResources::CronJob(ref cron) => {
                                        // What we currently care about is what the pod spec is
                                        // for the job. So we make a pseudo pod and use that to schedule
                                        let pod_spec = cron
                                            .spec
                                            .as_ref()
                                            .and_then(|s| s.job_template.spec.clone())
                                            .and_then(|s| s.template.spec);
                                        if let Some(pod_spec) = pod_spec {
                                            let pod = Pod {
                                                metadata: cron.metadata.clone(),
                                                spec: Some(pod_spec),
                                                status: None,
                                            };

                                            self.pod_scheduler.choose_node(&state.nodes, &pod)
                                        } else {
                                            return Err(anyhow!(
                                                "failed to find pod spec in cron job {}",
                                                cron.metadata.name.clone().unwrap_or_default()
                                            )
                                            .into());
                                        }
                                    }
                                    _ => {
                                        // we should not get here
                                        return Err(anyhow!(
                                            "internal scheduler error: found non pod resource {}:{} without a pre allocated node",
                                            op.resource,
                                            op.resource.name()
                                        ).into());
                                    }
                                }
                            }
                        };

                        if selection.selected.is_none() {
                            let reasons = selection
                                .rejected
                                .iter()
                                .map(|r| format!("{} - {}", r.node_name, r.reason))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let reasons = if reasons.is_empty() {
                                "<none>".to_string()
                            } else {
                                reasons
                            };

                            return Err(anyhow!(
                                "failed to find feasible node ({} rejected): {}",
                                selection.rejected.len(),
                                reasons
                            )
                            .into());
                        }

                        let node_name = selection.selected.unwrap().node_name.clone();

                        if dry_run {
                            let _ = state.reconcile_object_creation(&op.resource, &node_name)?;
                            if !op.silent {
                                println!(
                                    "{} {} {} created on node {}",
                                    op.operation.symbol(),
                                    &op.resource,
                                    &op.resource.name(),
                                    node_name
                                );
                            }
                            continue;
                        }

                        let client = conns.find(&node_name).unwrap();
                        let serialized = serde_yaml::to_string(&op.resource)
                            .expect("failed to serialize object");

                        match client.apply_resource(&serialized).await {
                            Ok((_, stderr)) => {
                                // if !stdout.trim().is_empty() {
                                //     stdout.trim().split("\n").for_each(|line| println!("{} - {}", node_name, line));
                                // }
                                if !stderr.is_empty() {
                                    stderr.trim().split("\n").for_each(|line| {
                                        eprintln!("{} - ERROR: {}", node_name, line)
                                    });
                                }
                                let _ =
                                    state.reconcile_object_creation(&op.resource, &node_name)?;

                                if !op.silent {
                                    println!(
                                        "{} {} {} created on node {}",
                                        op.operation.symbol(),
                                        op.resource,
                                        &op.resource.name(),
                                        node_name
                                    );
                                }
                                result.push(op.clone());
                            }
                            Err(err) => {
                                op.error = Some(err.to_string());
                                println!(
                                    "{} {} {} creation failed on node {}: {}",
                                    CROSS_EMOJI,
                                    op.resource,
                                    op.resource.name().name,
                                    node_name,
                                    err
                                );
                                result.push(op.clone());
                            }
                        }
                    }
                    OpType::Info => {
                        let node_name = op.node.clone().unwrap().node_name;

                        if !op.silent {
                            println!(
                                "{} {} on {}",
                                op.operation.symbol(),
                                op.resource.name(),
                                node_name
                            );
                        }
                        result.push(op.clone());
                    }
                    OpType::Unchanged => {
                        let node_name = op.node.clone().unwrap().node_name;

                        if !op.silent {
                            println!(
                                "{} {} {} unchanged on {}",
                                op.operation.symbol(),
                                op.resource,
                                op.resource.name(),
                                node_name
                            );
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    async fn schedule_one(
        &self,
        conns: &NodeClients,
        state: &mut ClusterState,
        object: SupportedResources,
        dry_run: bool,
    ) -> Result<Vec<ScheduledOperation>, Box<dyn Error>> {
        let plan = Self::plan(state, &object)?;
        if plan.actions.is_empty() {
            return Err(anyhow!("failed to schedule resources, no planned actions").into());
        }

        self.apply(plan, conns, state, dry_run).await
    }
}

#[async_trait(? Send)]
impl Scheduler for DefaultScheduler {
    async fn schedule(
        &self,
        conns: &NodeClients,
        state: &mut ClusterState,
        objects: Vec<SupportedResources>,
        dry_run: bool,
    ) -> Result<ScheduleResult, Box<dyn Error>> {
        let mut results = ScheduleResult { placements: vec![] };
        for object in objects {
            match self
                .schedule_one(conns, state, object.clone(), dry_run)
                .await
            {
                Ok(placements) => {
                    results.placements = [results.placements, placements].concat();
                }
                Err(err) => {
                    println!(
                        "{} failed to schedule {} {} : {}",
                        CROSS_EMOJI,
                        object,
                        object.name(),
                        err
                    );
                    results.placements = [
                        results.placements,
                        vec![ScheduledOperation::new(OpType::Info, object.clone())
                            .error(err.to_string())],
                    ]
                    .concat();
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use crate::test_helpers::objects::WithPod;
    use k8s_openapi::api::apps::v1::{DeploymentSpec, DeploymentStrategy};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::cmp::max;
    use std::collections::BTreeMap;

    #[test]
    fn test_plan_deployment_clean_slate_recreate() {
        let ns_name = NamespacedName {
            name: "foo".to_string(),
            namespace: "foo-namespace".to_string(),
        };

        let (_, deployment) = create_deployment_fixtures(&ns_name, 2, 0, "Recreate");

        let node1 = test_helpers::objects::node_state("node-1");
        let node2 = test_helpers::objects::node_state("node-2");

        let state = ClusterState {
            cluster_name: "test".to_string(),
            nodes: vec![node1, node2],
        };

        let result = DefaultScheduler::plan_deployment(&state, &deployment);
        assert!(result.is_ok());

        let result = &result.unwrap();
        assert_eq!(1, result.actions.len());

        let ops = result.actions.get(&ns_name);
        assert!(ops.is_some());

        let ops = ops.unwrap();
        assert_eq!(4, ops.len());

        let deployment_ops = ops
            .iter()
            .filter(|o| matches!(o.resource, SupportedResources::Deployment(_)))
            .collect_vec();
        assert_eq!(2, deployment_ops.len());
        assert!(deployment_ops.iter().all(|o| o.operation == OpType::Create
            && o.node.is_some()
            && !o.node.as_ref().unwrap().node_name.is_empty()));

        let pod_ops = ops
            .iter()
            .filter(|o| matches!(o.resource, SupportedResources::Pod(_)))
            .collect_vec();
        assert_eq!(2, pod_ops.len());
        assert!(pod_ops
            .iter()
            .all(|o| o.operation == OpType::Create && o.node.is_none()));

        println!(
            "{:?}",
            pod_ops.into_iter().map(|p| p.resource.name()).collect_vec()
        )
    }

    fn create_deployment_fixtures(
        ns_name: &NamespacedName,
        requested_replicas: usize,
        existing_replicas: usize,
        strategy: &str,
    ) -> (Vec<Pod>, Deployment) {
        let container = Container {
            args: Some(vec!["arg1".to_string()]),
            command: Some(vec!["cmd".to_string()]),
            name: "container1".to_string(),
            ..Default::default()
        };

        let mut pods = vec![];

        for i in 0..existing_replicas {
            let pod_name = format!("dpl-{}-{}", ns_name.name, i);

            let mut pod_meta = ObjectMeta {
                labels: Some(BTreeMap::from([
                    (SkateLabels::Deployment.to_string(), ns_name.name.clone()),
                    (SkateLabels::Name.to_string(), pod_name.clone()),
                    (
                        SkateLabels::Namespace.to_string(),
                        ns_name.namespace.clone(),
                    ),
                ])),
                ..Default::default()
            };
            pod_meta.name = Some(format!("{}.{}", pod_name, ns_name.namespace));
            let pod_spec = PodSpec {
                containers: vec![container.clone()],
                ..Default::default()
            };

            pods.push(Pod {
                metadata: pod_meta,
                spec: Some(pod_spec),
                status: None,
            })
        }

        let pod_template = PodTemplateSpec {
            metadata: Default::default(),
            spec: Some(PodSpec {
                containers: vec![],
                ..Default::default()
            }),
        };
        let deployment = Deployment {
            metadata: NamespacedName::new("foo", "foo-namespace").into(),
            spec: Some(DeploymentSpec {
                replicas: Some(requested_replicas as i32),
                template: pod_template.clone(),
                strategy: Some(DeploymentStrategy {
                    rolling_update: None,
                    type_: Some(strategy.to_string()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let sup_deployment = SupportedResources::Deployment(deployment);
        let sup_deployment = sup_deployment.fixup();
        assert!(sup_deployment.is_ok());
        let sup_deployment = sup_deployment.unwrap();

        let deployment = match sup_deployment {
            SupportedResources::Deployment(deployment) => deployment,
            _ => panic!("wrong type"),
        };

        (pods, deployment)
    }

    #[test]
    fn test_plan_deployment_recreate() {
        let existing_replicas = 2;
        let requested_replicas = 2;
        let ns_name = NamespacedName {
            name: "foo".to_string(),
            namespace: "foo-namespace".to_string(),
        };

        let (pods, deployment) =
            create_deployment_fixtures(&ns_name, requested_replicas, existing_replicas, "Recreate");

        let node1 = test_helpers::objects::node_state("node-1");
        let node2 = test_helpers::objects::node_state("node-2");

        let mut nodes = [node1, node2];

        for (i, pod) in pods.iter().enumerate().take(existing_replicas) {
            let node_index: usize = (i + 1) % 2; // 0 or 1 alternating
            nodes[node_index] = nodes[node_index].clone().with_pod(pod)
        }

        let state = ClusterState {
            cluster_name: "test".to_string(),
            nodes: vec![nodes[0].clone(), nodes[1].clone()],
        };

        let result = DefaultScheduler::plan_deployment(&state, &deployment);
        if result.is_err() {
            panic!("{}", result.err().unwrap())
        }

        let result = &result.unwrap();
        assert_eq!(1, result.actions.len());

        let ops = result.actions.get(&ns_name);
        assert!(ops.is_some());

        let ops = ops.unwrap();
        assert_eq!(2 + existing_replicas + requested_replicas, ops.len());

        let deployment_ops = ops
            .iter()
            .filter(|o| matches!(o.resource, SupportedResources::Deployment(_)))
            .collect_vec();
        assert_eq!(2, deployment_ops.len());
        assert!(deployment_ops.iter().all(|o| o.operation == OpType::Create
            && o.node.is_some()
            && !o.node.as_ref().unwrap().node_name.is_empty()));

        let pod_ops = ops
            .iter()
            .filter(|o| matches!(o.resource, SupportedResources::Pod(_)))
            .collect_vec();
        assert_eq!(4, pod_ops.len());

        assert_eq!(OpType::Delete, pod_ops[0].operation);
        assert_eq!(OpType::Delete, pod_ops[1].operation);
        assert_eq!(OpType::Create, pod_ops[2].operation);
        assert_eq!(OpType::Create, pod_ops[3].operation);
    }

    #[test]
    fn test_plan_deployment_rolling_update() {
        let existing_replicas = 2;
        let requested_replicas = 2;
        let ns_name = NamespacedName {
            name: "foo".to_string(),
            namespace: "foo-namespace".to_string(),
        };

        let (pods, deployment) = create_deployment_fixtures(
            &ns_name,
            requested_replicas,
            existing_replicas,
            "RollingUpdate",
        );

        let node1 = test_helpers::objects::node_state("node-1");
        let node2 = test_helpers::objects::node_state("node-2");

        let mut nodes = [node1, node2];

        for (i, pod) in pods.iter().enumerate().take(existing_replicas) {
            let node_index: usize = (i + 1) % 2; // 0 or 1 alternating
            nodes[node_index] = nodes[node_index].clone().with_pod(pod)
        }

        let state = ClusterState {
            cluster_name: "test".to_string(),
            nodes: vec![nodes[0].clone(), nodes[1].clone()],
        };

        let result = DefaultScheduler::plan_deployment(&state, &deployment);
        if result.is_err() {
            panic!("{}", result.err().unwrap())
        }

        let result = &result.unwrap();

        assert_eq!(
            1 + max(requested_replicas, existing_replicas),
            result.actions.len()
        );

        let deployment_ops = result
            .actions
            .get(&ns_name)
            .unwrap()
            .iter()
            .filter(|o| matches!(o.resource, SupportedResources::Deployment(_)))
            .collect_vec();

        assert_eq!(2, deployment_ops.len());

        assert!(deployment_ops.iter().all(|o| o.operation == OpType::Create
            && o.node.is_some()
            && !o.node.as_ref().unwrap().node_name.is_empty()));

        for pod in pods.iter().take(existing_replicas) {
            let pod_name = metadata_name(pod);
            let pod_ops = result.actions.get(&pod_name);
            assert!(pod_ops.is_some());

            let pod_ops = pod_ops.unwrap();
            assert_eq!(2, pod_ops.len());

            assert_eq!(OpType::Delete, pod_ops[0].operation);
            assert_eq!(OpType::Create, pod_ops[1].operation);
        }
    }
}
