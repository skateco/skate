use std::any::Any;
use std::cmp::Ordering;
use std::error::Error;
use anyhow::anyhow;
use async_trait::async_trait;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;


use crate::skate::SupportedResources;
use crate::skatelet::PodmanPodStatus;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState, NodeStatus};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI, hash_k8s_resource};


#[derive(Debug)]
pub struct ScheduleResult {
    pub placements: Vec<ScheduledOperation<SupportedResources>>,
}

#[async_trait(? Send)]
pub trait Scheduler {
    async fn schedule(&self, conns: SshClients, state: &ClusterState, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>>;
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

impl DefaultScheduler {
    fn choose_node(nodes: Vec<NodeState>, _object: &SupportedResources) -> Option<NodeState> {
        // filter nodes based on resource requirements  - cpu, memory, etc


        let filtered_nodes = nodes.iter().filter(|n| {
            n.status == NodeStatus::Healthy
                && n.host_info.as_ref().and_then(|h| {
                h.system_info.as_ref().and_then(|si| {
                    si.root_disk.as_ref().and_then(|rd| {
                        Some(
                            rd.available_space_b > 0
                        )
                    })
                })
            }).unwrap_or(false)
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

        feasible_node
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
        if deployment_pods.len() > replicas as usize {
            // cull the extra pods
            let for_removal: Vec<_> = deployment_pods.into_iter().filter_map(|(pod_info, node)| {
                let replica = pod_info.labels.clone().get("skate.io/replica").unwrap_or(&"0".to_string()).clone();
                let replica = replica.parse::<u32>().unwrap_or(0);
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


        // existing pods with same name (duplicates if more than 1)
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
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &ClusterState, object: &SupportedResources) -> Result<ApplyPlan, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => Self::plan_pod(state, pod),
            SupportedResources::Deployment(deployment) => Self::plan_deployment(state, deployment),
            SupportedResources::DaemonSet(_) => todo!("plan daemonset")
        }
    }

    async fn remove_existing(conns: &SshClients, resource: ScheduledOperation<SupportedResources>) -> Result<(), Box<dyn Error>> {
        let conn = conns.find(&resource.node.unwrap().node_name).ok_or("failed to find connection to host")?;

        let manifest = serde_yaml::to_string(&resource.resource).expect("failed to serialize manifest");
        match conn.remove_resource(&manifest).await {
            Ok(_) => Ok(()),
            Err(err) => Err(err)
        }
    }

    async fn schedule_one(conns: &SshClients, state: &ClusterState, object: SupportedResources) -> Result<Vec<ScheduledOperation<SupportedResources>>, Box<dyn Error>> {
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
                    let node_name = Self::choose_node(state.nodes.clone(), &action.resource).ok_or("failed to find feasible node")?.node_name.clone();
                    let client = conns.find(&node_name).unwrap();
                    let serialized = serde_yaml::to_string(&action.resource).expect("failed to serialize object");

                    match client.apply_resource(&serialized).await {
                        Ok(_) => {

                            // todo update state

                            println!("{} created {} on node {}", CHECKBOX_EMOJI, action.resource.name(), node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("{} failed to created {} on node {}: {}", CROSS_EMOJI, action.resource.name(), node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Info => {
                    let node_name = action.node.clone().unwrap().node_name;
                    println!("{} {} on {}", CHECKBOX_EMOJI, action.resource.name(), node_name);
                    result.push(action.clone());
                }
                OpType::Unchanged => {
                    let node_name = action.node.clone().unwrap().node_name;
                    println!("{} {} on {} unchanged", CHECKBOX_EMOJI, action.resource.name(), node_name);
                }
            }
        }

        Ok(result)
    }
}

#[async_trait(? Send)]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, conns: SshClients, state: &ClusterState, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>> {
        let mut results = ScheduleResult { placements: vec![] };
        for object in objects {
            match object {
                SupportedResources::Pod(_) | SupportedResources::Deployment(_) => {
                    match Self::schedule_one(&conns, state, object.clone()).await {
                        Ok(placements) => {
                            results.placements = [results.placements, placements].concat();
                        }
                        Err(err) => {
                            results.placements = [results.placements, vec![ScheduledOperation {
                                resource: object.clone(),
                                node: None,
                                operation: OpType::Info,
                                error: Some(err.to_string()),
                            }]].concat()
                        }
                    }
                },
                SupportedResources::DaemonSet(_) => todo!("schedule daemonset")
            }
        }
        Ok(results)
    }
}
