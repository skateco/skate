use crate::get::lister::{Lister, NameFilters};
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::state::state::ClusterState;
use k8s_openapi::api::core::v1::Node;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct NodeLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeListItem {
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub pods: String,
    #[serde(skip)]
    pub status: String,
    #[serde(skip)]
    pub message: String,
    #[tabled(skip)]
    #[serde(flatten)]
    pub manifest: serde_yaml::Value,
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

                let k8s_node: Node = n.into();
                NodeListItem {
                    name: n.node_name.clone(),
                    pods: num_pods.to_string(),
                    status: n.status.to_string(),
                    message: n.message.clone().unwrap_or_default(),
                    manifest: serde_yaml::to_value(k8s_node).unwrap_or(serde_yaml::Value::Null),
                }
            })
            .collect()
    }
}
