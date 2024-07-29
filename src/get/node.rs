use std::collections::HashMap;
use std::error::Error;
use chrono::{Local, SecondsFormat};
use itertools::Itertools;
use crate::get::{get_objects, GetArgs, GetObjectArgs, IdCommand, Lister};
use crate::get::deployment::DeploymentLister;
use crate::skatelet::{PodmanPodInfo, PodmanPodStatus, SystemInfo};
use crate::state::state::{ClusterState, NodeState};




pub (crate) struct NodeLister {}

impl Lister<NodeState> for NodeLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<NodeState>> {
        todo!()
    }

    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<NodeState> {
        state.nodes.iter().filter(|n| {
            match filters.clone().id {
                Some(id) => match id {
                    IdCommand::Id(ids) => {
                        ids.first().unwrap_or(&"".to_string()).clone() == n.node_name
                    }
                }
                _ => true
            }
        }).map(|n| n.clone()).collect()
    }

    fn print(&self, items: Vec<NodeState>) {
        println!(
            "{0: <30}  {1: <10}  {2: <10}",
            "NAME", "PODS", "STATUS"
        );
        for node in items {
            let num_pods = match node.host_info {
                Some(hi) => match hi.system_info {
                    Some(si) => match si.pods {
                        Some(pods) => pods.len(),
                        _ => 0
                    }
                    _ => 0
                }
                _ => 0
            };
            println!(
                "{0: <30}  {1: <10}  {2: <10}",
                node.node_name, num_pods, node.status
            )
        }
    }
}

