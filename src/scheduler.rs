use std::cmp::Ordering;
use std::error::Error;
use std::hash::Hasher;
use anyhow::anyhow;
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::config::Node;
use crate::skate::SupportedResources;
use crate::ssh::{SshClients};
use crate::state::state::{ClusterState, NodeState, NodeStatus};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI, hash_k8s_resource, hash_string};


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
        let mut d = d.clone();

        let replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
        let mut actions = vec!();

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

        let name = d.metadata.name.clone().unwrap_or("".to_string());
        let ns = d.metadata.namespace.clone().unwrap_or("".to_string());
        let mut existing_pods: Vec<_> = state.locate_deployment(&name, &ns).map(|(r, n)| {
            r.iter().map(|pod| {
                let replica = pod.labels.clone().get("skate.io/replica").unwrap_or(&"0".to_string()).clone();
                let replica = replica.parse::<u32>().unwrap_or(0);

                (replica, ScheduledOperation {
                    node: Some(n.clone()),
                    resource: SupportedResources::Pod(pod.clone().into()),
                    error: None,
                    operation: OpType::Delete,
                })
            }).collect()
        }).unwrap_or_default();

        let mut for_removal = vec!();

        if existing_pods.len() > replicas as usize {
            // cull the extra pods
            for_removal = existing_pods.into_iter().filter_map(|(replica, op)| {
                if replica >= replicas as u32 {
                    return Some(op);
                }
                None
            }).collect();
        }

        Ok((ApplyPlan {
            actions: [actions, for_removal].concat()
        }))
    }
    fn plan_pod(state: &ClusterState, object: &Pod) -> Result<ApplyPlan, Box<dyn Error>> {
        let mut pod = object.clone();
        //let feasible_node = Self::choose_node(state.nodes.clone(), &SupportedResources::Pod(object.clone())).ok_or("failed to find feasible node")?;


        let hash = hash_k8s_resource(&mut pod);

        let name = pod.metadata.name.clone().unwrap_or("".to_string());
        let ns = pod.metadata.namespace.clone().unwrap_or("".to_string());
        let existing_pod = state.locate_pod(&name, &ns);

        let for_removal = match existing_pod.as_ref() {
            Some((pod_info, node)) => {
                let previous_hash = pod_info.labels.get("skate.io/hash").unwrap_or(&"".to_string()).clone();
                match previous_hash.clone() != hash {
                    false => None,
                    true => Some(ScheduledOperation {
                        node: Some(node.clone().clone()),
                        resource: SupportedResources::Pod(pod_info.clone().into()),
                        error: None,
                        operation: OpType::Delete,
                    })
                }
            }
            _ => None
        };
        let for_removal: Vec<_> = vec!(for_removal).into_iter().filter_map(|item| item).collect();

        if existing_pod.is_some() && for_removal.len() == 0 {
            // nothing to do
            return Ok(ApplyPlan {
                actions: vec!()
            });
        }


        let to_create = ScheduledOperation {
            resource: SupportedResources::Pod(pod),
            node: None, // set later //Some(feasible_node.clone()),
            operation: OpType::Create,
            error: None,
        };

        Ok(ApplyPlan {
            actions: [for_removal, vec!(to_create)].concat()
        })
    }
    // returns tuple of (Option(prev node), Option(new node))
    fn plan(state: &ClusterState, object: &SupportedResources) -> Result<ApplyPlan, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => Self::plan_pod(state, pod),
            SupportedResources::Deployment(deployment) => Self::plan_deployment(state, deployment)
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
                    let node_name = Self::choose_node(state.nodes.clone(), &action.resource).ok_or("failed to find feasible node")?.node_name.clone();
                    let client = conns.find(&node_name).unwrap();
                    let serialized = serde_yaml::to_string(&action.resource).expect("failed to serialize object");

                    match client.apply_resource(&serialized).await {
                        Ok(_) => {

                            // todo update state

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
                    let node_name = action.node.clone().unwrap().node_name;
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
