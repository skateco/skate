use crate::get::lister::{Lister, NameFilters};
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::state::state::ClusterState;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct NodeLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeListItem {
    pub name: String,
    pub pods: String,
    pub status: String,
    pub message: String,
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
    fn list(
        &self,
        _: ResourceType,
        filters: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<NodeListItem> {
        state
            .nodes
            .iter()
            .filter(|n| filters.id.is_none() || filters.id.clone().unwrap() == n.node_name)
            .map(|n| {
                let num_pods = match n.host_info.as_ref() {
                    Some(hi) => match hi.system_info.as_ref() {
                        Some(si) => match si.pods.as_ref() {
                            Some(pods) => pods.len(),
                            _ => 0,
                        },
                        _ => 0,
                    },
                    _ => 0,
                };
                NodeListItem {
                    name: n.node_name.clone(),
                    pods: num_pods.to_string(),
                    status: n.status.to_string(),
                    message: n.message.clone().unwrap_or_default(),
                }
            })
            .collect()
    }
}
