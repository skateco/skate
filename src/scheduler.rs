use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use itertools::Itertools;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Node as K8sNode, Pod};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::Metadata;


use crate::skate::SupportedResources;
use crate::skatelet::PodmanPodStatus;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI, EQUAL_EMOJI, hash_k8s_resource, INFO_EMOJI};


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
    // pub existing: Vec<ScheduledOperation<SupportedResources>>,
    pub actions: Vec<ScheduledOperation<SupportedResources>>,
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
                s.unschedulable.and_then(|u| Some(!u))
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
                return matches;
            })
        }).collect::<Vec<_>>();


        let feasible_node = filtered_nodes.into_iter().fold(None, |maybe_prev_node, node| {
            let node_pods = node.clone().host_info.and_then(|h| {
                h.system_info.and_then(|si| {
                    si.pods.and_then(|p| Some(p.len()))
                })
            }).unwrap_or(0);

            maybe_prev_node.and_then(|prev_node: NodeState| {
                prev_node.host_info.clone().and_then(|h| {
                    h.system_info.and_then(|si| {
                        si.pods.and_then(|prev_pods| {
                            match prev_pods.len().cmp(&node_pods) {
                                Ordering::Less => Some(prev_node.clone()),
                                Ordering::Equal => Some(node.clone()),
                                Ordering::Greater => Some(node.clone()),
                            }
                        })
                    })
                })
            }).or_else(|| Some(node.clone()))
        });

        (feasible_node, rejected_nodes)
    }

    fn plan_daemonset(state: &ClusterState, ds: &DaemonSet) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut actions = vec!();

        let _name = ds.metadata.name.clone().unwrap_or("".to_string());
        let _ns = ds.metadata.namespace.clone().unwrap_or("".to_string());

        for node in state.nodes.iter() {
            let node_name = node.node_name.clone();
            let mut pod_spec = ds.spec.clone().and_then(|s| Some(s.template)).and_then(|t| t.spec).unwrap_or_default();

            let mut meta = ds.spec.as_ref().and_then(|s| s.template.metadata.clone()).unwrap_or_default();
            meta.name = Some(format!("{}-{}", ds.metadata.name.as_ref().unwrap(), node_name));
            meta.namespace = ds.metadata.namespace.clone();

            let mut labels = meta.labels.clone().unwrap_or_default();
            labels.insert("skate.io/daemonset".to_string(), ds.metadata.name.as_ref().unwrap().clone());
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
            actions.extend(result.actions);
        }

        Ok(ApplyPlan {
            actions: [actions].concat()
        })
    }

    fn plan_deployment(state: &ClusterState, d: &Deployment) -> Result<ApplyPlan, Box<dyn Error>> {
        let d = d.clone();

        let replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
        let mut actions = vec!();

        let name = d.metadata.name.clone().unwrap_or("".to_string());
        let ns = d.metadata.namespace.clone().unwrap_or("".to_string());
        // check if  there are more pods than replicas running
        // cull them if so
        let deployment_pods = state.locate_deployment(&name, &ns);

        let deployment_pods: Vec<_> = deployment_pods.into_iter().map(|(dp, node)| {
            let replica = dp.labels.get("skate.io/replica").unwrap_or(&"0".to_string()).clone();
            let replica = replica.parse::<u32>().unwrap_or(0);
            (dp, node, replica)
        }).sorted_by_key(|(_, _, replica)| replica.clone()).rev().collect();

        if deployment_pods.len() > replicas as usize {
            // cull the extra pods
            let for_removal: Vec<_> = deployment_pods.into_iter().filter_map(|(pod_info, node, replica)| {
                if replica >= replicas as u32 {
                    return Some(ScheduledOperation {
                        node: Some(node.clone()),
                        resource: SupportedResources::Pod(pod_info.clone().into()),
                        error: None,
                        operation: OpType::Delete,
                    });
                }
                None
            }).collect();

            actions.extend(for_removal);
        }


        for i in 0..replicas {
            let pod_spec = d.spec.clone().and_then(|s| Some(s.template)).and_then(|t| t.spec).unwrap_or_default();

            let mut meta = d.spec.as_ref().and_then(|s| s.template.metadata.clone()).unwrap_or_default();
            meta.name = Some(format!("{}-{}", d.metadata.name.as_ref().unwrap(), i));
            meta.namespace = d.metadata.namespace.clone();
            let mut labels = meta.labels.unwrap_or_default();
            labels.insert("skate.io/deployment".to_string(), d.metadata.name.as_ref().unwrap().clone());
            labels.insert("skate.io/replica".to_string(), i.to_string());
            meta.labels = Some(labels.clone());
            meta.labels = Some(labels);

            let pod = Pod {
                metadata: meta,
                spec: Some(pod_spec),
                status: None,
            };


            let result = Self::plan_pod(state, &pod)?;
            actions.extend(result.actions);
        }


        Ok(ApplyPlan {
            actions: [actions].concat()
        })
    }
    fn plan_pod(state: &ClusterState, object: &Pod) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut new_pod = object.clone();
        //let feasible_node = Self::choose_node(state.nodes.clone(), &SupportedResources::Pod(object.clone())).ok_or("failed to find feasible node")?;


        let new_hash = hash_k8s_resource(&mut new_pod);

        let name = new_pod.metadata.name.clone().unwrap_or("".to_string());
        let ns = new_pod.metadata.namespace.clone().unwrap_or("".to_string());

        // smuggle node selectors as labels
        match new_pod.spec.as_ref() {
            Some(spec) => {
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
            None => {}
        }


        // existing pods with same name (duplicates if more than 1)
        // sort by replicas descending
        let existing_pods = state.locate_pods(&name, &ns);


        let cull_actions: Vec<_> = match existing_pods.len() {
            0 => vec!(),
            1 => vec!(),
            _ => (&existing_pods.as_slice()[1..]).iter().map(|(pod_info, node)| {
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


        Ok(ApplyPlan {
            actions: [cull_actions, actions].concat()
        })
    }

    fn plan_cronjob(_state: &ClusterState, cron: &CronJob) -> Result<ApplyPlan, Box<dyn Error>> {
        // TODO - check with current state
        // Sanitise manifest since we'll be running that later via kube play
        // - only 1 replica
        let mut actions = vec!();

        actions.push(ScheduledOperation {
            resource: SupportedResources::CronJob(cron.clone()),
            error: None,
            operation: OpType::Create,
            node: None,
        });

        Ok(ApplyPlan {
            actions,
        })
    }

    fn plan_ingress(state: &ClusterState, ingress: &Ingress) -> Result<ApplyPlan, Box<dyn Error>> {
        // TODO - check with current state
        // TODO - warn about unsupported settings
        let mut actions = vec!();

        for node in state.nodes.iter() {
            actions.push(ScheduledOperation {
                node: Some(node.clone()),
                resource: SupportedResources::Ingress(ingress.clone()),
                error: None,
                operation: OpType::Create,
            });
        }

        Ok(ApplyPlan {
            actions,
        })
    }
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &mut ClusterState, object: &SupportedResources) -> Result<ApplyPlan, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => Self::plan_pod(state, pod),
            SupportedResources::Deployment(deployment) => Self::plan_deployment(state, deployment),
            SupportedResources::DaemonSet(ds) => Self::plan_daemonset(state, ds),
            SupportedResources::Ingress(ingress) => Self::plan_ingress(state, ingress),
            SupportedResources::CronJob(cron) => Self::plan_cronjob(state, cron),
        }
    }

    async fn remove_existing(conns: &SshClients, resource: ScheduledOperation<SupportedResources>) -> Result<(), Box<dyn Error>> {
        let conn = conns.find(&resource.node.unwrap().node_name).ok_or("failed to find connection to host")?;

        let manifest = serde_yaml::to_string(&resource.resource).expect("failed to serialize manifest");
        match conn.remove_resource_by_manifest(&manifest).await {
            Ok(_) => Ok(()),
            Err(err) => Err(err)
        }
    }

    async fn schedule_one(conns: &SshClients, state: &mut ClusterState, object: SupportedResources) -> Result<Vec<ScheduledOperation<SupportedResources>>, Box<dyn Error>> {
        let plan = Self::plan(state, &object)?;
        if plan.actions.len() == 0 {
            return Err(anyhow!("failed to schedule resources").into());
        }

        let mut result: Vec<ScheduledOperation<SupportedResources>> = vec!();

        for mut action in plan.actions {
            match action.operation {
                OpType::Delete => {
                    let node_name = action.node.clone().unwrap().node_name;

                    match Self::remove_existing(conns, action.clone()).await {
                        Ok(_) => {
                            println!("{} deleted {} on node {} ", CHECKBOX_EMOJI, action.resource.name(), node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("{} failed to delete {} on node {}: {}", CROSS_EMOJI, action.resource.name(), node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Create => {
                    let (node, rejected_nodes) = match action.node.clone() {
                        // some things like ingress have the node already set
                        Some(n) => (Some(n), vec!()),
                        // anything else and things with node selectors go here
                        None => Self::choose_node(state.nodes.clone(), &action.resource)
                    };
                    if !node.is_some() {
                        let reasons = rejected_nodes.iter().map(|r| format!("{} - {}", r.node_name, r.reason)).collect::<Vec<_>>().join(", ");
                        return Err(anyhow!("failed to find feasible node: {}", reasons).into());
                    }

                    let node_name = node.unwrap().node_name.clone();

                    let client = conns.find(&node_name).unwrap();
                    let serialized = serde_yaml::to_string(&action.resource).expect("failed to serialize object");

                    match client.apply_resource(&serialized).await {
                        Ok((stdout, stderr)) => {
                            println!("{}{}", stdout, stderr);
                            let _ = state.reconcile_object_creation(&action.resource, &node_name)?;
                            println!("{} created {} on node {}", CHECKBOX_EMOJI, &action.resource.name(), node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("{} failed to create {} on node {}: {}", CROSS_EMOJI, action.resource.name().name, node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Info => {
                    let node_name = action.node.clone().unwrap().node_name;
                    println!("{} {} on {}", INFO_EMOJI, action.resource.name(), node_name);
                    result.push(action.clone());
                }
                OpType::Unchanged => {
                    let node_name = action.node.clone().unwrap().node_name;
                    println!("{} {} on {} unchanged", EQUAL_EMOJI, action.resource.name(), node_name);
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
            match Self::schedule_one(&conns, state, object.clone()).await {
                Ok(placements) => {
                    results.placements = [results.placements, placements].concat();
                }
                Err(err) => {
                    println!("{} failed to schedule {} : {}", CROSS_EMOJI, object.name(), err.to_string());
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
