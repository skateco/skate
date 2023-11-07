use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use strum_macros::Display;
use crate::config::{cache_dir, Config};
use crate::skate::SupportedResources;
use crate::ssh::HostInfoResponse;
use crate::state::state::NodeStatus::{Healthy, Unhealthy, Unknown};
use crate::util::{hash_string, slugify};

#[derive(Serialize, Deserialize, Clone, Display)]
pub enum NodeStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NodeState {
    pub node_name: String,
    pub status: NodeStatus,
    pub inventory_found: bool,
    pub inventory: Vec<SupportedResources>,
}

#[derive(Serialize, Deserialize)]
pub struct State {
    pub cluster_name: String,
    pub hash: String,
    pub nodes: Vec<NodeState>,
    pub orphaned_nodes: Option<Vec<NodeState>>,
}

pub struct ReconciledResult {
    pub orphaned_nodes: usize,
    pub new_nodes: usize,
}

impl State {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(State::path(&self.cluster_name.clone()).as_str())).expect("unable to open state file");
        Ok(serde_json::to_writer(state_file, self).expect("failed to write json state"))
    }

    pub fn load(cluster_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(State::path(cluster_name))
            .expect("file should open read only");
        let result: State = serde_json::from_reader(file).expect("failed to decode state");
        Ok(result)
    }

    pub fn reconcile(&mut self, config: &Config, host_info: &Vec<HostInfoResponse>) -> Result<ReconciledResult, Box<dyn Error>> {
        let cluster = config.current_cluster()?;
        self.hash = hash_string(cluster);

        let state_hosts: HashSet<String> = self.nodes.iter().map(|n| n.node_name.clone()).collect();
        let config_hosts: HashSet<String> = cluster.nodes.iter().map(|n| n.name.clone()).collect();


        let new = &config_hosts - &state_hosts;
        let orphaned = &state_hosts - &config_hosts;

        let mut new_nodes: Vec<NodeState> = config.current_cluster()?.nodes.iter().filter_map(|n| {
            match new.contains(&n.name) {
                true => Some(NodeState {
                    node_name: n.name.clone(),
                    status: Unknown,
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
        self.nodes = self.nodes.iter().map(|mut node| {
            let mut node = node.clone();
            match host_info.iter().find(|h| h.node_name == node.node_name) {
                Some(_) => {
                    node.status = Healthy;
                }
                None => {
                    node.status = Unhealthy;
                }
            };
            node
        }).collect();


        Ok(ReconciledResult {
            orphaned_nodes: orphaned_len,
            new_nodes: new_len,
        })
    }
}
