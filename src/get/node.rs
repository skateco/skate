use tabled::Tabled;
use crate::get::{GetObjectArgs, Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::{SystemInfo};
use crate::state::state::ClusterState;


pub(crate) struct NodeLister {}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeListItem {
    pub name: String,
    pub pods: String,
    pub status: String,
}

impl NameFilters for NodeListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        "".to_string()
    }
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

}

