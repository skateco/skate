use std::collections::{BTreeMap, HashSet};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::ops::DerefMut;
use std::path::Path;
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::core::v1::{NodeSpec, NodeStatus as K8sNodeStatus, Node as K8sNode, NodeAddress};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta};
use strum_macros::Display;
use crate::config::{cache_dir, Config};

use crate::skate::SupportedResources;
use crate::skatelet::PodmanPodInfo;
use crate::ssh::NodeSystemInfo;
use crate::state::state::NodeStatus::{Healthy, Unhealthy, Unknown};
use crate::util::{hash_string, slugify};

#[derive(Serialize, Deserialize, Clone, Debug, Display, PartialEq)]
pub enum NodeStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeState {
    pub node_name: String,
    pub status: NodeStatus,
    pub host_info: Option<NodeSystemInfo>,
}

impl Into<K8sNode> for NodeState {
    fn into(self) -> K8sNode {
        let mut metadata = ObjectMeta::default();
        let mut spec = NodeSpec::default();
        let mut status = K8sNodeStatus::default();

        metadata.name = Some(self.node_name.clone());
        metadata.namespace = Some("default".to_string());
        metadata.uid = Some(self.node_name.clone());

        status.phase = match self.status {
            Unknown => Some("Pending".to_string()),
            Healthy => Some("Ready".to_string()),
            Unhealthy => Some("Pending".to_string()),
        };

        spec.unschedulable = match self.status {
            Unknown => Some(true),
            Healthy => Some(false),
            Unhealthy => Some(true),
        };

        let sys_info = self.host_info.as_ref().and_then(|h| h.system_info.clone());


        (status.capacity, status.allocatable, status.addresses, metadata.labels) = match sys_info {
            Some(si) => {
                (Some(BTreeMap::<String, Quantity>::from([
                    ("cpu".to_string(), Quantity(format!("{}", si.num_cpus))),
                    ("memory".to_string(), Quantity(format!("{} Mib", si.total_memory_mib))),
                ])),
                 (Some(BTreeMap::<String, Quantity>::from([
                     ("cpu".to_string(), Quantity(format!("{}", (si.num_cpus as f32) * (100.00 - si.cpu_usage) / 100.0))),
                     ("memory".to_string(), Quantity(format!("{} Mib", si.total_memory_mib - si.used_memory_mib))),
                 ]))), ({
                    let mut addresses = vec![
                        NodeAddress {
                            address: si.hostname.clone(),
                            type_: "Hostname".to_string(),
                        },
                    ];
                    match si.external_ip_address {
                        Some(ip) => {
                            addresses.push(NodeAddress {
                                address: ip,
                                type_: "ExternalIP".to_string(),
                            })
                        }
                        None => {}
                    }
                    match si.internal_ip_address {
                        Some(ip) => {
                            addresses.push(NodeAddress {
                                address: ip,
                                type_: "InternalIP".to_string(),
                            })
                        }
                        None => {}
                    }
                    Some(addresses)
                }), (
                     Some(BTreeMap::<String, String>::from([
                         ("skate.io/arch".to_string(), si.platform.arch.clone()),
                         ("skate.io/os".to_string(), si.platform.os.to_string().to_lowercase()),
                         ("skate.io/hostname".to_string(), si.hostname.clone()),
                     ]))
                 ))
            }
            None => (None, None, None, None)
        };


        K8sNode {
            metadata,
            spec: Some(spec),
            status: Some(status),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClusterState {
    pub cluster_name: String,
    pub hash: String,
    pub nodes: Vec<NodeState>,
}

pub struct ReconciledResult {
    pub removed: usize,
    pub added: usize,
    pub updated: usize,
}

impl ClusterState {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(ClusterState::path(&self.cluster_name.clone()).as_str()))
            .map_err(|e| anyhow!("failed to open or create state file").context(e))?;
        serde_json::to_writer(state_file, self)
            .map_err(|e| anyhow!("failed to serialize state").context(e))?;
        Ok(())
    }

    pub fn load(cluster_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(ClusterState::path(cluster_name))?;

        let result: ClusterState = serde_json::from_reader(file)?;
        Ok(result)
    }

    pub fn reconcile_node(&mut self, node: &NodeSystemInfo) -> Result<ReconciledResult, Box<dyn Error>> {
        let pos = self.nodes.iter_mut().find_position(|n| n.node_name == node.node_name);

        let result = match pos {
            Some((p, _obj)) => {
                self.nodes[p] = (*node).clone().into();
                ReconciledResult {
                    removed: 0,
                    added: 0,
                    updated: 1,
                }
            }
            None => {
                self.nodes.push((*node).clone().into());
                ReconciledResult {
                    removed: 0,
                    added: 1,
                    updated: 0,
                }
            }
        };

        Ok(result)
    }

    pub fn reconcile_object_creation(&mut self, object: &SupportedResources, node_name: &str) -> Result<ReconciledResult, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => self.reconcile_pod_creation(&PodmanPodInfo::from((*pod).clone()), node_name),
            _ => todo!("reconcile not supported")
        }
    }
    pub fn reconcile_pod_creation(&mut self, pod: &PodmanPodInfo, node_name: &str) -> Result<ReconciledResult, Box<dyn Error>> {
        let mut node = self.nodes.iter_mut().find(|n| n.node_name == node_name)
            .ok_or(anyhow!("node not found: {}", node_name))?;

        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().and_then(|pods| {
                    pods.push(pod.clone());
                    Some(())
                })
            })
        });

        Ok(ReconciledResult {
            removed: 0,
            added: 1,
            updated: 0,
        })
    }
    pub fn reconcile_all_nodes(&mut self, config: &Config, host_info: &Vec<NodeSystemInfo>) -> Result<ReconciledResult, Box<dyn Error>> {
        let cluster = config.current_cluster()?;
        self.hash = hash_string(cluster);

        let state_hosts: HashSet<String> = self.nodes.iter().map(|n| n.node_name.clone()).collect();

        let config_hosts: HashSet<String> = cluster.nodes.iter().map(|n| n.name.clone()).collect();


        let new = &config_hosts - &state_hosts;
        let orphaned = &state_hosts - &config_hosts;


        self.nodes = self.nodes.iter().filter_map(|n| {
            match orphaned.contains(&n.node_name) {
                false => Some(n.clone()),
                true => None
            }
        }).collect();

        let mut new_nodes: Vec<NodeState> = config.current_cluster()?.nodes.iter().filter_map(|n| {
            match new.contains(&n.name) {
                true => Some(NodeState {
                    node_name: n.name.clone(),
                    status: Unknown,
                    host_info: None,
                }),
                false => None
            }
        }).collect();

        self.nodes.append(&mut new_nodes);


        let mut updated = 0;
        // now that we have our list, go through and mark them healthy or unhealthy
        self.nodes = self.nodes.iter().map(|node| {
            let mut node = node.clone();
            match host_info.iter().find(|h| h.node_name == node.node_name) {
                Some(info) => {
                    updated = updated + 1;
                    node.status = match info.healthy() {
                        true => Healthy,
                        false => Unhealthy
                    };
                    node.host_info = Some(info.clone())
                }
                None => {
                    node.status = Unknown;
                }
            };
            node
        }).collect();


        Ok(ReconciledResult {
            removed: orphaned.len(),
            added: new.len(),
            updated,
        })
    }

    pub fn filter_pods(&self, f: &dyn Fn(&PodmanPodInfo) -> bool) -> Vec<(PodmanPodInfo, &NodeState)> {
        let res: Vec<_> = self.nodes.iter().filter_map(|n| {
            n.host_info.as_ref().and_then(|h| {
                h.system_info.clone().and_then(|i| {
                    i.pods.and_then(|p| {
                        Some(p.clone().into_iter().filter(|p| f(p)).map(|p| vec!((p, n))).collect::<Vec<_>>())
                    })
                })
            })
        }).flatten().flatten().collect();
        res
    }

    pub fn locate_daemonset(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        self.filter_pods(&|p| p.name == name && p.namespace() == namespace && p.labels.get("skate.io/daemonset").is_some())
    }

    pub fn locate_pods(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        self.filter_pods(&|p| p.name == name && p.namespace() == namespace)
    }

    pub fn locate_deployment(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        let name = name.strip_prefix(format!("{}.", namespace).as_str()).unwrap_or(name);
        self.filter_pods(&|p| p.deployment() == name && p.namespace() == namespace)
    }
}
