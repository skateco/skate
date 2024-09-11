use tabled::Tabled;
use crate::get::{GetObjectArgs, Lister};

use crate::skatelet::{SystemInfo};
use crate::state::state::{ClusterState, NodeState};


pub(crate) struct NodeLister {}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeListItem {
    pub name: String,
    pub pods: String,
    pub status: String,
}

impl Lister<NodeListItem> for NodeLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Vec<NodeListItem> {
        unimplemented!("not used")
    }

    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<NodeListItem> {
        state.nodes.iter().filter(|n| filters.id.is_none() || filters.id.clone().unwrap() == n.node_name).map(|n| {
            let num_pods = match n.host_info.as_ref() {
                Some(hi) => match hi.system_info.as_ref() {
                    Some(si) => match si.pods.as_ref() {
                        Some(pods) => pods.len(),
                        _ => 0
                    }
                    _ => 0
                }
                _ => 0
            };
            NodeListItem {
                name: n.node_name.clone(),
                pods: num_pods.to_string(),
                status: n.status.to_string(),
            }
        }).collect()
    }

    // fn print(&self, items: Vec<NodeState>) {
    //     println!(
    //         "{0: <30}  {1: <10}  {2: <10}",
    //         "NAME", "PODS", "STATUS"
    //     );
    //     for node in items {
    //         let num_pods = match node.host_info {
    //             Some(hi) => match hi.system_info {
    //                 Some(si) => match si.pods {
    //                     Some(pods) => pods.len(),
    //                     _ => 0
    //                 }
    //                 _ => 0
    //             }
    //             _ => 0
    //         };
    //         println!(
    //             "{0: <30}  {1: <10}  {2: <10}",
    //             node.node_name, num_pods, node.status
    //         )
    //     }
    // }
}

