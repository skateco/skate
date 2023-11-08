use std::cmp::Ordering;
use std::error::Error;
use async_trait::async_trait;
use crate::config::Cluster;
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::SupportedResources;
use crate::ssh::{HostInfoResponse, SshClients};
use crate::state::state::{ClusterState, NodeState};


#[derive(Debug)]
pub enum Status {
    Scheduled(String),
    Error(String),
}

#[derive(Debug)]
pub struct ScheduleResult {
    pub object: SupportedResources,
    pub node_name: String,
    pub status: Status,
}

#[async_trait]
pub trait Scheduler {
    async fn schedule(&self, conns: SshClients, state: &ClusterState, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>>;
}

pub struct DefaultScheduler {}

impl DefaultScheduler {
    fn pick_node(state: &ClusterState, object: &SupportedResources) -> Option<NodeState> {
        // naive - picks node with fewest pods
        state.nodes.iter().fold(None, |a, c| {
            let current_node_pods = match c.clone().host_info {
                Some(hi) => {
                    match hi.system_info {
                        Some(si) => {
                            match si.pods {
                                Some(pods) => pods.len(),
                                None => 0
                            }
                        }
                        _ => 0
                    }
                }
                _ => 0
            };

            match a.clone() {
                Some(node) => {
                    match node.host_info {
                        Some(hi) => {
                            match hi.system_info {
                                Some(si) => {
                                    match si.pods {
                                        Some(pods) => {
                                            match pods.len().cmp(&current_node_pods) {
                                                Ordering::Less => a,
                                                Ordering::Equal => Some(c.clone()),
                                                Ordering::Greater => Some(c.clone()),
                                            }
                                        }
                                        None => a
                                    }
                                }
                                None => a
                            }
                        }
                        None => a
                    }
                }
                None => a
            }
        })
    }

    async fn schedule_one(conns: &SshClients, state: &ClusterState, object: SupportedResources) -> ScheduleResult {
        let serialized = serde_yaml::to_string(&object);
        let serialized = match serialized {
            Ok(serialized) => serialized,
            Err(err) => return ScheduleResult {
                object,
                node_name: "".to_string(),
                status: ScheduleError(format!("{}", err)),
            }
        };

        let node = Self::pick_node(state, &object);
        let node = match node {
            Some(node) => node,
            None => return ScheduleResult {
                object,
                node_name: "".to_string(),
                status: ScheduleError("failed to find schedulable node".to_string()),
            }
        };

        let client = conns.find(&node.node_name).unwrap();


        println!("scheduling {} on node {}", object, node.node_name.clone());
        let result = client.apply_resource(&serialized).await;
        ScheduleResult {
            object,
            node_name: node.node_name.clone(),
            status: match result {
                Ok((stdout, stderr)) => {
                    let mut builder = String::new();
                    builder.push_str(&stdout);
                    if stderr.len() > 0 {
                        builder.push_str(&format!(" ( stderr: {} )", stderr))
                    }
                    Scheduled(format!("{}", builder.replace("\n", "\\n")))
                }
                Err(err) => ScheduleError(err.to_string())
            },
        }
    }
}

#[async_trait]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, conns: SshClients, state: &ClusterState, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>> {
        let node_name = &state.nodes.first().ok_or("no nodes")?.node_name;

        let client = conns.find(node_name).ok_or("failed to find connection for node")?;

        let mut results: Vec<ScheduleResult> = vec![];
        for object in objects {
            match object {
                SupportedResources::Pod(_) | SupportedResources::Deployment(_) => {
                    let result = Self::schedule_one(&conns, state, object.clone()).await;
                    results.push(result)
                }
            }
        }
        Ok(results)
    }
}
