use std::cmp::Ordering;
use std::error::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::config::Node;
use crate::skate::SupportedResources;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState, NodeStatus};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};


#[derive(Debug)]
pub enum Status {
    Scheduled(String),
    Error(String),
}

#[derive(Debug)]
pub struct ScheduleResult {
    pub placements: Vec<ScheduledOperation<SupportedResources>>,
}

#[async_trait(? Send)]
pub trait Scheduler {
    async fn schedule(&self, conns: SshClients, state: &ClusterState, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>>;
}

pub struct DefaultScheduler {}


#[derive(Debug, Clone)]
pub enum OpType {
    Info,
    Create,
    Delete,
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
    fn choose_node(nodes: Vec<NodeState>, object: &SupportedResources) -> Option<NodeState> {
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


        filtered_nodes.into_iter().fold(None, |maybe_prev_node, node| {
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
        })
    }
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &ClusterState, object: &SupportedResources) -> Result<ApplyPlan, Box<dyn Error>> {
        let for_deletion: Vec<_> = match object {
            SupportedResources::Pod(p) => {
                let name = p.metadata.name.clone().unwrap_or("".to_string());
                let ns = p.metadata.namespace.clone().unwrap_or("".to_string());
                state.locate_pod(&name, &ns).map(|(pod, n)| {
                    vec!(ScheduledOperation {
                        node: Some(n.clone()),
                        resource: SupportedResources::Pod(pod.clone().into()),
                        error: None,
                        operation: OpType::Delete,
                    })
                }).unwrap_or_default()
            }
            SupportedResources::Deployment(d) => {
                let name = d.metadata.name.clone().unwrap_or("".to_string());
                let ns = d.metadata.namespace.clone().unwrap_or("".to_string());
                state.locate_deployment(&name, &ns).map(|(r, n)| {
                    r.iter().map(|pod| {
                        ScheduledOperation {
                            node: Some(n.clone()),
                            resource: SupportedResources::Pod(pod.clone().into()),
                            error: None,
                            operation: OpType::Delete,
                        }
                    }).collect()
                }).unwrap_or_default()
            }
        };


        let for_creation = match object {
            SupportedResources::Pod(p) => {
                let feasible_node = Self::choose_node(state.nodes.clone(), object).ok_or("failed to find feasible node")?;
                vec!(ScheduledOperation {
                    resource: object.clone(),
                    node: Some(feasible_node.clone()),
                    operation: OpType::Create,
                    error: None,
                })
            }
            SupportedResources::Deployment(d) => {
                let replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
                let mut pods = vec!();
                for i in 0..replicas {
                    let feasible_node = Self::choose_node(state.nodes.clone(), object).ok_or("failed to find feasible node")?;
                    let pod_spec = d.spec.clone().and_then(|s| Some(s.template)).and_then(|t| t.spec).unwrap_or_default();

                    let mut meta = d.spec.as_ref().and_then(|s| s.template.metadata.clone()).unwrap_or_default();
                    meta.name = Some(format!("{}-{}", d.metadata.name.as_ref().unwrap(), i));
                    meta.namespace = d.metadata.namespace.clone();
                    let mut labels = meta.labels.unwrap_or_default();
                    labels.insert("skate.io/deployment".to_string(), d.metadata.name.as_ref().unwrap().clone());
                    meta.labels = Some(labels);
                    let pod = Pod {
                        metadata: meta,
                        spec: Some(pod_spec),
                        status: None,
                    };
                    pods.push(ScheduledOperation {
                        resource: SupportedResources::Pod(pod),
                        node: Some(feasible_node.clone()),
                        operation: OpType::Create,
                        error: None,
                    });
                }
                pods
            }
        };

        Ok(ApplyPlan {
            actions: [for_deletion, for_creation].concat(),
        })
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
            let node_name = action.node.clone().unwrap().node_name;
            match action.operation {
                OpType::Delete => {
                    match Self::remove_existing(conns, action.clone()).await {
                        Ok(_) => {
                            println!("{} deleted {} on node {} ", CHECKBOX_EMOJI, object, node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("{} failed to delete {} on node {}: {}", CROSS_EMOJI, object, node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Create => {
                    let client = conns.find(&node_name).unwrap();
                    let serialized = serde_yaml::to_string(&action.resource).expect("failed to serialize object");

                    match client.apply_resource(&serialized).await {
                        Ok(_) => {
                            println!("{} created {} on node {}", CHECKBOX_EMOJI, object, node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("{} failed to created {} on node {}: {}", CROSS_EMOJI, object, node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Info => {
                    println!("{} {} on {}", CHECKBOX_EMOJI, object, node_name);
                    result.push(action.clone());
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
                }
            }
        }
        Ok(results)
    }
}
