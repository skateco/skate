use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use strum_macros::Display;
use crate::config::{cache_dir, Config};
use crate::skate::SupportedResources;
use crate::skatelet::PodmanPodInfo;
use crate::ssh::HostInfoResponse;
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
    pub host_info: Option<HostInfoResponse>,
    pub inventory_found: bool,
    pub inventory: Vec<SupportedResources>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClusterState {
    pub cluster_name: String,
    pub hash: String,
    pub nodes: Vec<NodeState>,
    pub orphaned_nodes: Option<Vec<NodeState>>,
}

pub struct ReconciledResult {
    pub orphaned_nodes: usize,
    pub new_nodes: usize,
}

impl ClusterState {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(ClusterState::path(&self.cluster_name.clone()).as_str())).expect("unable to open state file");
        Ok(serde_json::to_writer(state_file, self).expect("failed to write json state"))
    }

    pub fn load(cluster_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(ClusterState::path(cluster_name))?;

        let result: ClusterState = serde_json::from_reader(file)?;
        Ok(result)
    }

    pub fn reconcile(&mut self, config: &Config, host_info: &Vec<HostInfoResponse>) -> Result<ReconciledResult, Box<dyn Error>> {
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
                    inventory_found: false,
                    inventory: vec![],
                }),
                false => None
            }
        }).collect();

        self.nodes.append(&mut new_nodes);

        let orphaned_nodes: Vec<_> = self.nodes.iter().filter_map(|n| match orphaned.contains(&n.node_name) {
            true => Some((*n).clone()),
            false => None
        }).collect();
        let orphaned_len = orphaned_nodes.len();
        let new_len = new.len();
        self.orphaned_nodes = Some(orphaned_nodes);

        // now that we have our list, go through and mark them healthy or unhealthy
        self.nodes = self.nodes.iter().map(|node| {
            let mut node = node.clone();
            match host_info.iter().find(|h| h.node_name == node.node_name) {
                Some(info) => {
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
            orphaned_nodes: orphaned_len,
            new_nodes: new_len,
        })
    }

    pub fn locate_pod(&self, name: &str, namespace: &str) -> Option<(PodmanPodInfo, &NodeState)> {
        self.nodes.iter().find_map(|n| {
            match n.host_info.clone() {
                Some(h) => match h.system_info {
                    Some(i) => match i.pods {
                        Some(i) => {
                            let pod = i.iter().find(|p| p.name == name && p.namespace() == namespace);
                            match pod {
                                Some(pod) => Some((pod.clone(), n)),
                                None => None
                            }
                        }
                        None => None
                    }
                    None => None
                }
                None => None
            }
        })
    }

    pub fn locate_deployment(&self, name: &str, namespace: &str) -> Option<(Vec<PodmanPodInfo>, &NodeState)> {
        let name = name.strip_prefix(format!("{}.", namespace).as_str()).unwrap_or(name);
        self.nodes.iter().find_map(|n| {
            match n.host_info.clone() {
                Some(h) => match h.system_info {
                    Some(i) => match i.pods {
                        Some(i) => {
                            let pods: Vec<_> = i.into_iter().filter(|p| &p.deployment() == name && &p.namespace() == namespace).collect();
                            match pods.len() {
                                0 => None,
                                _ => Some((pods, n)),
                            }
                        }
                        None => None
                    }
                    None => None
                }
                None => None
            }
        })
    }
}
