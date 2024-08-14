use crate::get::{GetObjectArgs , Lister};

use crate::skatelet::{SystemInfo};
use crate::state::state::{ClusterState, NodeState};


pub(crate) struct NodeLister {}

impl Lister<NodeState> for NodeLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Option<Vec<NodeState>> {
        todo!()
    }

    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<NodeState> {
        state.nodes.iter().filter(|n| filters.id.is_none() || filters.id.clone().unwrap() == n.node_name).map(|n| n.clone()).collect()
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

