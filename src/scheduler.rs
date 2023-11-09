use std::cmp::Ordering;
use std::error::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use crate::skate::SupportedResources;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState};


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
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &ClusterState, object: &SupportedResources) -> ApplyPlan {
        // only support 1 replica for now
        match object {
            SupportedResources::Deployment(d) => {
                d.spec.as_ref().and_then(|spec| spec.replicas.and_then(|mut replicas| {
                    if replicas > 1 {
                        return Some(1);
                    }
                    Some(replicas)
                }));
            }
            _ => {}
        };

        let for_deletion: Vec<_> = match object {
            SupportedResources::Pod(p) => {
                let name = p.metadata.name.clone().unwrap_or("".to_string());
                let ns = p.metadata.namespace.clone().unwrap_or("".to_string());
                state.locate_pod(&name, &ns).map(|(pod, n)| {
                    vec!(ScheduledOperation {
                        node: Some(n.clone()),
                        resource: SupportedResources::Pod(pod.clone().into()),
                        error: None,
                        operation: OpType::Info,
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
                            operation: OpType::Info,
                        }
                    }).collect()
                }).unwrap_or_default()
            }
        };
        // TODO - different node for each pod in deployment potentially
        // naive - picks node with fewest pods
        let scheduled_node = state.nodes.iter().fold(None, |maybe_prev_node, node| {
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

        let for_creation = match scheduled_node {
            Some(n) => vec!(ScheduledOperation {
                resource: object.clone(),
                node: Some(n),
                operation: OpType::Create,
                error: None,
            }),
            _ => vec!()
        };

        ApplyPlan {
            actions: [for_deletion, for_creation].concat(),
        }
    }

    async fn remove_existing(conns: &SshClients, resource: ScheduledOperation<SupportedResources>) -> Result<(), Box<dyn Error>> {
        let conn = conns.find(&resource.node.unwrap().node_name).ok_or("failed to find connection to host")?;

        let mut success = true;
        let mut error: String = "".to_string();
        let manifest = serde_yaml::to_string(&resource.resource).expect("failed to serialize manifest");
        match conn.remove_resource(&manifest).await {
            Ok(_) => {
                println!("removed existing resource")
            }
            Err(err) => {
                success = false;
                error = error + &format!("{}", err)
            }
        }
        if !success {
            return Err(anyhow!(error).into());
        }
        Ok(())
    }

    async fn schedule_one(conns: &SshClients, state: &ClusterState, object: SupportedResources) -> Result<Vec<ScheduledOperation<SupportedResources>>, Box<dyn Error>> {
        let serialized = serde_yaml::to_string(&object).expect("failed to serialize object");

        let plan = Self::plan(state, &object);
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
                            println!("deleted {} on node {}", object, node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("failed to delete {} on node {}: {}", object, node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Create => {
                    let node_name = action.node.clone().unwrap().node_name;
                    let client = conns.find(&node_name).unwrap();
                    match client.apply_resource(&serialized).await {
                        Ok(_) => {
                            println!("created {} on node {}", object, node_name);
                            result.push(action.clone());
                        }
                        Err(err) => {
                            action.error = Some(err.to_string());
                            println!("failed to created {} on node {}: {}", object, node_name, err.to_string());
                            result.push(action.clone());
                        }
                    }
                }
                OpType::Info => {
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
