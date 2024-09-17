use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use itertools::Itertools;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, RollingUpdateDeployment};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Node as K8sNode, Pod, Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::Metadata;


use crate::skate::SupportedResources;
use crate::skatelet::system::podman::PodmanPodStatus;
use crate::spec::cert::ClusterIssuer;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI, EQUAL_EMOJI, hash_k8s_resource, INFO_EMOJI, metadata_name, NamespacedName};


#[derive(Debug)]
pub struct ScheduleResult {
    pub placements: Vec<ScheduledOperation<SupportedResources>>,
}

#[async_trait(? Send)]
pub trait Scheduler {
    async fn schedule(&self, conns: &SshClients, state: &mut ClusterState, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>>;
}

pub struct DefaultScheduler {}


#[derive(Debug, Clone, PartialEq)]
pub enum OpType {
    Info,
    Create,
    Delete,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct ScheduledOperation<T> {
    pub resource: T,
    pub node: Option<NodeState>,
    pub operation: OpType,
    pub error: Option<String>,
}

pub struct ApplyPlan {
    pub actions: HashMap<NamespacedName, Vec<ScheduledOperation<SupportedResources>>>,
}

pub struct RejectedNode {
    pub node_name: String,
    pub reason: String,
}

impl DefaultScheduler {
    fn choose_node(nodes: Vec<NodeState>, object: &SupportedResources) -> (Option<NodeState>, Vec<RejectedNode>) {
        // filter nodes based on resource requirements  - cpu, memory, etc

        let node_selector = match object {
            SupportedResources::Pod(pod) => {
                pod.spec.as_ref().and_then(|s| {
                    s.node_selector.clone()
                })
            }
            _ => None
        }.unwrap_or(BTreeMap::new());

        let mut rejected_nodes: Vec<RejectedNode> = vec!();


        let filtered_nodes = nodes.iter().filter(|n| {
            let k8s_node: K8sNode = (**n).clone().into();
            let node_labels = k8s_node.metadata.labels.unwrap_or_default();
            // only schedulable nodes
            let is_schedulable = k8s_node.spec.and_then(|s| {
                s.unschedulable.map(|u| !u)
            }).unwrap_or(false);

            if !is_schedulable {
                rejected_nodes.push(RejectedNode {
                    node_name: n.node_name.clone(),
                    reason: "node is unschedulable".to_string(),
                });
                return false;
            }

            // only nodes that match the nodeselectors
            node_selector.iter().all(|(k, v)| {
                let matches = node_labels.get(k).unwrap_or(&"".to_string()) == v;
                if !matches {
                    rejected_nodes.push(RejectedNode {
                        node_name: n.node_name.clone(),
                        reason: format!("node selector {}:{} did not match", k, v),
                    });
                }
                matches
            })
        }).collect::<Vec<_>>();


        let feasible_node = filtered_nodes.into_iter().fold(None, |maybe_prev_node, node| {
            let node_pods = node.clone().host_info.and_then(|h| {
                h.system_info.and_then(|si| {
                    si.pods.map(|p| p.len())
                })
            }).unwrap_or(0);

            maybe_prev_node.and_then(|prev_node: NodeState| {
                prev_node.host_info.clone().and_then(|h| {
                    h.system_info.and_then(|si| {
                        si.pods.map(|prev_pods| {
                            match prev_pods.len().cmp(&node_pods) {
                                Ordering::Less => prev_node.clone(),
                                Ordering::Equal => node.clone(),
                                Ordering::Greater => node.clone(),
                            }
                        })
                    })
                })
            }).or_else(|| Some(node.clone()))
        });

        (feasible_node, rejected_nodes)
    }

    fn plan_daemonset(state: &ClusterState, ds: &DaemonSet) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut actions = HashMap::new();

        let daemonset_name = ds.metadata.name.clone().unwrap_or("".to_string());
        let ns = ds.metadata.namespace.clone().unwrap_or("".to_string());

        for node in state.nodes.iter() {
            let node_name = node.node_name.clone();
            let mut pod_spec = ds.spec.clone().map(|s| s.template).and_then(|t| t.spec).unwrap_or_default();

            // inherit daemonset labels
            let mut meta = ds.spec.as_ref().and_then(|s| s.template.metadata.clone()).unwrap_or_default();

            let name = format!("dms-{}-{}", daemonset_name.clone(), node_name);

            let ns_name = NamespacedName { name: name.clone(), namespace: ns.clone() };

            meta.name = Some(ns_name.to_string());
            meta.namespace = Some(ns.clone());

            let mut labels = meta.labels.clone().unwrap_or_default();
            labels.insert("skate.io/name".to_string(), name.clone());
            labels.insert("skate.io/daemonset".to_string(), daemonset_name.clone());
            meta.labels = Some(labels);

            // bind to specific node
            pod_spec.node_selector = Some({
                let mut selector = pod_spec.node_selector.unwrap_or_default();
                selector.insert("skate.io/nodename".to_string(), node_name.clone());
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

        Ok(ApplyPlan {
            actions
        })
    }

    fn plan_deployment(state: &ClusterState, d: &Deployment) -> Result<ApplyPlan, Box<dyn Error>> {

        // check if  there are more pods than replicas running
        // cull them if so
        let strategy = d.spec.clone().unwrap_or_default().strategy.unwrap_or_default();

        let is_rolling = match strategy.type_.clone().unwrap_or_default().as_str() {
            "RollingUpdate" => {
                strategy.rolling_update.is_some()
            }
            _ => false
        };

        if is_rolling {
            return Self::plan_deployment_rolling_update(state, d, strategy.rolling_update.unwrap());
        }
        Self::plan_deployment_recreate(state, d)
    }

    fn plan_deployment_rolling_update(state: &ClusterState, d: &Deployment, ru: RollingUpdateDeployment) -> Result<ApplyPlan, Box<dyn Error>> {
        let actions = Self::plan_deployment_recreate(state, d)?;

        // TODO - respect max surge and max unavailable
        // will require parallelism.
        // And an OpType::Parallel

        Ok(actions)

        //
    }

    fn plan_deployment_recreate(state: &ClusterState, d: &Deployment) -> Result<ApplyPlan, Box<dyn Error>> {
        let plan = Self::plan_deployment_generic(state, d)?;

        // have one top level key, with all the actions, sorted by delete, create, unchanged

        let mut new_actions = vec!();

        for (k, v) in plan.actions {
            new_actions.extend(v)
        }


        new_actions = new_actions.into_iter().sorted_by(|a, b| {
            match a.operation {
                OpType::Delete => {
                    Ordering::Less
                }
                OpType::Create => {
                    Ordering::Greater
                }
                _ => {
                    Ordering::Equal
                }
            }
        }).collect();

        Ok(ApplyPlan{actions: HashMap::from([(metadata_name(d), new_actions)])})
    }

    fn plan_deployment_generic(state: &ClusterState, d: &Deployment) -> Result<ApplyPlan, Box<dyn Error>> {
        let d = d.clone();

        let replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
        let mut actions: HashMap<_, Vec<_>> = HashMap::new();

        let deployment_name = d.metadata.name.clone().unwrap_or("".to_string());
        let ns = d.metadata.namespace.clone().unwrap_or("".to_string());

        let existing_pods = state.locate_deployment(&deployment_name, &ns);

        let existing_pods: Vec<_> = existing_pods.into_iter().map(|(dp, node)| {
            let replica = dp.labels.get("skate.io/replica").unwrap_or(&"0".to_string()).clone();
            let replica = replica.parse::<u32>().unwrap_or(0);
            (dp, node, replica)
        }).sorted_by_key(|(_, _, replica)| *replica).rev().collect();

        if existing_pods.len() > replicas as usize {
            // cull the extra pods
            for (pod_info, node, replica) in existing_pods {
                if replica >= replicas as u32 {
                    let pod: Pod = pod_info.into();
                    let name = NamespacedName::from(pod.metadata.name.clone().unwrap_or_default().as_str());
                    let op = ScheduledOperation {
                        node: Some(node.clone()),
                        resource: SupportedResources::Pod(pod),
                        error: None,
                        operation: OpType::Delete,
                    };
                    match actions.get_mut(&name) {
                        Some(ops) => ops.push(op),
                        None => {
                            actions.insert(name, vec!(op));
                        }
                    };
                }
            }
        }


        for i in 0..replicas {
            let pod_spec = d.spec.clone().map(|s| s.template).and_then(|t| t.spec).unwrap_or_default();

            // inherit deployment labels
            let mut meta = d.spec.as_ref().and_then(|s| s.template.metadata.clone()).unwrap_or_default();
            // name format needs to be <type>.<fqn>.<replica>
            let name = format!("dpl-{}-{}", deployment_name,  i);
            let ns_name = NamespacedName { name: name.clone(), namespace: ns.clone() };
            // needs to be the fqn for kube play, since that's what it'll call the pod
            meta.name = Some(ns_name.to_string());
            meta.namespace = Some(ns.clone());

            let mut labels = meta.labels.unwrap_or_default();
            labels.insert("skate.io/name".to_string(), name);
            labels.insert("skate.io/deployment".to_string(), deployment_name.clone());
            labels.insert("skate.io/replica".to_string(), i.to_string());
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


        Ok(ApplyPlan {
            actions
        })
    }

    fn plan_pod(state: &ClusterState, object: &Pod) -> Result<Vec<ScheduledOperation<SupportedResources>>, Box<dyn Error>> {
        let mut new_pod = object.clone();
        //let feasible_node = Self::choose_node(state.nodes.clone(), &SupportedResources::Pod(object.clone())).ok_or("failed to find feasible node")?;

        let new_hash = hash_k8s_resource(&mut new_pod);

        let name = metadata_name(object);

        // smuggle node selectors as labels
        if let Some(spec) = new_pod.spec.as_ref() {
            if spec.node_selector.is_some() {
                let ns = spec.node_selector.clone().unwrap();
                let selector_labels = ns.iter().map(|(k, v)| {
                    (format!("nodeselector/{}", k), v.clone())
                });
                let mut labels = new_pod.metadata().labels.clone().unwrap_or_default();
                labels.extend(selector_labels);
                new_pod.metadata_mut().labels = Some(labels)
            }
        }


        // existing pods with same name (duplicates if more than 1)
        // sort by replicas descending
        let existing_pods = state.locate_pods(&name.name, &name.namespace);


        let cull_actions: Vec<_> = match existing_pods.len() {
            0 | 1 => vec!(),
            _ => existing_pods.as_slice()[1..].iter().map(|(pod_info, node)| {
                ScheduledOperation {
                    node: Some((**node).clone()),
                    resource: SupportedResources::Pod(pod_info.clone().into()),
                    error: None,
                    operation: OpType::Delete,
                }
            }).collect(),
        };

        let existing_pod = &existing_pods.first();


        let actions = match existing_pod {
            Some((pod_info, node)) => {
                let previous_hash = pod_info.labels.get("skate.io/hash").unwrap_or(&"".to_string()).clone();
                let state_running = pod_info.status == PodmanPodStatus::Running;

                let hash_matches = previous_hash.clone() == new_hash;
                match hash_matches && state_running {
                    true => vec!(ScheduledOperation {
                        node: Some((**node).clone()),
                        resource: SupportedResources::Pod(pod_info.clone().into()),
                        error: None,
                        operation: OpType::Unchanged,
                    }),
                    false => {
                        vec!(
                            ScheduledOperation {
                                node: Some((**node).clone()),
                                resource: SupportedResources::Pod(pod_info.clone().into()),
                                error: None,
                                operation: OpType::Delete,
                            },
                            ScheduledOperation {
                                node: None,
                                resource: SupportedResources::Pod(new_pod),
                                error: None,
                                operation: OpType::Create,
                            }
                        )
                    }
                }
            }
            None => vec!(
                ScheduledOperation {
                    node: None,
                    resource: SupportedResources::Pod(new_pod.clone()),
                    error: None,
                    operation: OpType::Create,
                }
            )
        };


        Ok([cull_actions, actions].concat())
    }

    fn plan_cronjob(state: &ClusterState, cron: &CronJob) -> Result<ApplyPlan, Box<dyn Error>> {
        let name = metadata_name(cron);

        let mut new_cron = cron.clone();

        // Sanitise manifest since we'll be running that later via kube play
        // - only 1 replica
        let mut actions = vec!();

        let new_hash = hash_k8s_resource(&mut new_cron);


        let existing_cron = state.locate_objects(None, |si| {
            si.clone().cronjobs
        }, &name.name, &name.namespace).first().cloned();


        match existing_cron {
            Some(c) => {
                if c.0.manifest_hash == new_hash {
                    actions.push(ScheduledOperation {
                        resource: SupportedResources::CronJob(new_cron),
                        error: None,
                        operation: OpType::Unchanged,
                        node: Some(c.1.clone()),
                    });
                    // nothing to do
                } else {
                    actions.push(ScheduledOperation {
                        resource: SupportedResources::CronJob(new_cron.clone()),
                        error: None,
                        operation: OpType::Delete,
                        node: Some(c.1.clone()),
                    });

                    actions.push(ScheduledOperation {
                        resource: SupportedResources::CronJob(new_cron),
                        error: None,
                        operation: OpType::Create,
                        node: None,
                    });
                }
            }
            None => {
                actions.push(ScheduledOperation {
                    resource: SupportedResources::CronJob(new_cron),
                    error: None,
                    operation: OpType::Create,
                    node: None,
                });
            }
        }


        // check if we have an existing cronjob for this
        // if so compare hashes, if differ then create, otherwise no change


        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    // just apply on all nodes
    fn plan_secret(state: &ClusterState, secret: &Secret) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut actions = vec!();
        let ns_name = metadata_name(secret);

        for node in state.nodes.iter() {
            actions.extend([
                ScheduledOperation {
                    resource: SupportedResources::Secret(secret.clone()),
                    error: None,
                    operation: OpType::Create,
                    node: Some(node.clone()),
                }
            ]);
        }


        Ok(ApplyPlan {
            actions: HashMap::from([(ns_name, actions)]),
        })
    }
    fn plan_service(state: &ClusterState, service: &Service) -> Result<ApplyPlan, Box<dyn Error>> {
        let name = metadata_name(service);

        let mut actions = vec!();


        let mut new_service = service.clone();

        let new_hash = hash_k8s_resource(&mut new_service);


        for node in state.nodes.iter() {
            let existing_service = state.locate_objects(Some(&node.node_name), |si| {
                si.clone().services
            }, &name.name, &name.namespace).first().cloned();

            match existing_service {
                Some(c) => {
                    if c.0.manifest_hash == new_hash {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Service(new_service.clone()),
                            error: None,
                            operation: OpType::Unchanged,
                            node: Some(node.clone()),
                        });
                        // nothing to do
                    } else {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Service(new_service.clone()),
                            error: None,
                            operation: OpType::Delete,
                            node: Some(node.clone()),
                        });

                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Service(new_service.clone()),
                            error: None,
                            operation: OpType::Create,
                            node: Some(node.clone()),
                        });
                    }
                }
                None => {
                    actions.push(ScheduledOperation {
                        resource: SupportedResources::Service(new_service.clone()),
                        error: None,
                        operation: OpType::Create,
                        node: Some(node.clone()),
                    });
                }
            }
        }


        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    fn plan_ingress(state: &ClusterState, ingress: &Ingress) -> Result<ApplyPlan, Box<dyn Error>> {

        // TODO - warn about unsupported settings
        let mut actions = vec!();

        let mut new_ingress = ingress.clone();

        let new_hash = hash_k8s_resource(&mut new_ingress);

        let name = metadata_name(ingress);

        for node in state.nodes.iter() {
            let existing_ingress = state.locate_objects(Some(&node.node_name), |si| {
                si.clone().ingresses
            }, &name.name, &name.namespace).first().cloned();

            match existing_ingress {
                Some(c) => {
                    if c.0.manifest_hash == new_hash {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Ingress(new_ingress.clone()),
                            error: None,
                            operation: OpType::Unchanged,
                            node: Some(node.clone()),
                        });
                        // nothing to do
                    } else {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Ingress(new_ingress.clone()),
                            error: None,
                            operation: OpType::Delete,
                            node: Some(node.clone()),
                        });

                        actions.push(ScheduledOperation {
                            resource: SupportedResources::Ingress(new_ingress.clone()),
                            error: None,
                            operation: OpType::Create,
                            node: Some(node.clone()),
                        });
                    }
                }
                None => {
                    actions.push(ScheduledOperation {
                        resource: SupportedResources::Ingress(new_ingress.clone()),
                        error: None,
                        operation: OpType::Create,
                        node: Some(node.clone()),
                    });
                }
            }
        }

        Ok(ApplyPlan {
            actions: HashMap::from([(name, actions)]),
        })
    }

    fn plan_cluster_issuer(state: &mut ClusterState, cluster_issuer: &ClusterIssuer) -> Result<ApplyPlan, Box<dyn Error>> {
        let ns_name = metadata_name(cluster_issuer);

        let mut actions = vec!();


        let mut new_cluster_issuer = cluster_issuer.clone();

        let new_hash = hash_k8s_resource(&mut new_cluster_issuer);


        for node in state.nodes.iter() {
            let existing = state.locate_objects(Some(&node.node_name), |si| {
                si.clone().cluster_issuers
            }, &ns_name.name, "skate").first().cloned();

            match existing {
                Some(c) => {
                    if c.0.manifest_hash == new_hash {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::ClusterIssuer(new_cluster_issuer.clone()),
                            error: None,
                            operation: OpType::Unchanged,
                            node: Some(node.clone()),
                        });
                        // nothing to do
                    } else {
                        actions.push(ScheduledOperation {
                            resource: SupportedResources::ClusterIssuer(new_cluster_issuer.clone()),
                            error: None,
                            operation: OpType::Delete,
                            node: Some(node.clone()),
                        });

                        actions.push(ScheduledOperation {
                            resource: SupportedResources::ClusterIssuer(new_cluster_issuer.clone()),
                            error: None,
                            operation: OpType::Create,
                            node: Some(node.clone()),
                        });
                    }
                }
                None => {
                    actions.push(ScheduledOperation {
                        resource: SupportedResources::ClusterIssuer(new_cluster_issuer.clone()),
                        error: None,
                        operation: OpType::Create,
                        node: Some(node.clone()),
                    });
                }
            }
        }


        Ok(ApplyPlan {
            actions: HashMap::from([(ns_name, actions)]),
        })
    }
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &mut ClusterState, object: &SupportedResources) -> Result<ApplyPlan, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => {
                let ns_name = NamespacedName { name: pod.metadata.name.clone().unwrap_or_default(), namespace: pod.metadata.namespace.clone().unwrap_or_default() };
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

    async fn remove_existing(conns: &SshClients, resource: ScheduledOperation<SupportedResources>) -> Result<(String, String), Box<dyn Error>> {
        let hook_result = resource.resource.pre_remove_hook(resource.node.as_ref().unwrap(), conns).await;


        let conn = conns.find(&resource.node.unwrap().node_name).ok_or("failed to find connection to host")?;

        let manifest = serde_yaml::to_string(&resource.resource).expect("failed to serialize manifest");
        let remove_result = conn.remove_resource_by_manifest(&manifest).await;

        if hook_result.is_err() {
            return Err(hook_result.err().unwrap());
        }

        remove_result
    }

    async fn schedule_one(conns: &SshClients, state: &mut ClusterState, object: SupportedResources) -> Result<Vec<ScheduledOperation<SupportedResources>>, Box<dyn Error>> {
        let plan = Self::plan(state, &object)?;
        if plan.actions.is_empty() {
            return Err(anyhow!("failed to schedule resources, no planned actions").into());
        }

        let mut result: Vec<ScheduledOperation<SupportedResources>> = vec!();

        for (_name, ops) in plan.actions {
            for mut op in ops {
                match op.operation {
                    OpType::Delete => {
                        let node_name = op.node.clone().unwrap().node_name;

                        match Self::remove_existing(conns, op.clone()).await {
                            Ok((_, stderr)) => {
                                // println!("{}", stdout.trim());
                                if !stderr.is_empty() {
                                    eprintln!("{}", stderr.trim())
                                }
                                println!("{} {} {} deleted on node {} ", CHECKBOX_EMOJI, op.resource, op.resource.name(), node_name);
                                result.push(op.clone());
                            }
                            Err(err) => {
                                op.error = Some(err.to_string());
                                println!("{} failed to delete {} on node {}: {}", CROSS_EMOJI, op.resource.name(), node_name, err);
                                result.push(op.clone());
                            }
                        }
                    }
                    OpType::Create => {
                        let (node, rejected_nodes) = match op.node.clone() {
                            // some things like ingress have the node already set
                            Some(n) => (Some(n), vec!()),
                            // anything else and things with node selectors go here
                            None => Self::choose_node(state.nodes.clone(), &op.resource)
                        };
                        if node.is_none() {
                            let reasons = rejected_nodes.iter().map(|r| format!("{} - {}", r.node_name, r.reason)).collect::<Vec<_>>().join(", ");
                            return Err(anyhow!("failed to find feasible node: {}", reasons).into());
                        }

                        let node_name = node.unwrap().node_name.clone();

                        let client = conns.find(&node_name).unwrap();
                        let serialized = serde_yaml::to_string(&op.resource).expect("failed to serialize object");

                        match client.apply_resource(&serialized).await {
                            Ok((_, stderr)) => {
                                // if !stdout.trim().is_empty() {
                                //     stdout.trim().split("\n").for_each(|line| println!("{} - {}", node_name, line));
                                // }
                                if !stderr.is_empty() {
                                    stderr.trim().split("\n").for_each(|line| eprintln!("{} - ERROR: {}", node_name, line));
                                }
                                let _ = state.reconcile_object_creation(&op.resource, &node_name)?;
                                println!("{} {} {} created on node {}", CHECKBOX_EMOJI, op.resource, &op.resource.name(), node_name);
                                result.push(op.clone());
                            }
                            Err(err) => {
                                op.error = Some(err.to_string());
                                println!("{} {} {} creation failed on node {}: {}", CROSS_EMOJI, op.resource, op.resource.name().name, node_name, err);
                                result.push(op.clone());
                            }
                        }
                    }
                    OpType::Info => {
                        let node_name = op.node.clone().unwrap().node_name;
                        println!("{} {} on {}", INFO_EMOJI, op.resource.name(), node_name);
                        result.push(op.clone());
                    }
                    OpType::Unchanged => {
                        let node_name = op.node.clone().unwrap().node_name;
                        println!("{} {} {} unchanged on {}", EQUAL_EMOJI, op.resource, op.resource.name(), node_name);
                    }
                }
            }
        }

        Ok(result)
    }
}

#[async_trait(? Send)]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, conns: &SshClients, state: &mut ClusterState, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>> {
        let mut results = ScheduleResult { placements: vec![] };
        for object in objects {
            match Self::schedule_one(conns, state, object.clone()).await {
                Ok(placements) => {
                    results.placements = [results.placements, placements].concat();
                }
                Err(err) => {
                    println!("{} failed to schedule {} : {}", CROSS_EMOJI, object.name(), err);
                    results.placements = [results.placements, vec![ScheduledOperation {
                        resource: object.clone(),
                        node: None,
                        operation: OpType::Info,
                        error: Some(err.to_string()),
                    }]].concat()
                }
            }
        }
        Ok(results)
    }
}
