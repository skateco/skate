use crate::get::lister::{Lister, NameFilters};
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::PodmanPodInfo;
use crate::skatelet::SystemInfo;
use crate::state::state::ClusterState;
use crate::util::age;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct PodLister {}

impl NameFilters for &PodmanPodInfo {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn name(&self) -> String {
        self.labels
            .get("skate.io/name")
            .cloned()
            .unwrap_or("".to_string())
    }

    fn namespace(&self) -> String {
        self.labels
            .get("skate.io/namespace")
            .cloned()
            .unwrap_or("".to_string())
    }
}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct PodListItem {
    pub namespace: String,
    pub name: String,
    pub ready: String,
    pub status: String,
    pub restarts: String,
    pub age: String,
}

impl From<&PodmanPodInfo> for PodListItem {
    fn from(pod: &PodmanPodInfo) -> Self {
        let num_containers = pod.containers.clone().unwrap_or_default().len();
        let healthy_containers = pod
            .containers
            .clone()
            .unwrap_or_default()
            .iter()
            .filter(|c| matches!(c.status.as_str(), "running"))
            .collect::<Vec<_>>()
            .len();
        let restarts = pod
            .containers
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|c| c.restart_count.unwrap_or_default())
            .reduce(|a, c| a + c)
            .unwrap_or_default();

        PodListItem {
            namespace: pod.namespace(),
            name: pod.name(),
            ready: format!("{}/{}", healthy_containers, num_containers),
            status: pod.status.to_string(),
            restarts: restarts.to_string(),
            age: age(pod.created),
        }
    }
}

impl NameFilters for PodListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.to_string()
    }
}

impl Lister<PodListItem> for PodLister {
    fn list(
        &self,
        _: ResourceType,
        args: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<PodListItem> {
        let name = args.id.clone().unwrap_or_default();
        let namespace = args.namespace.clone().unwrap_or_default();

        state
            .nodes
            .iter()
            .filter_map(|n| {
                let pods: Vec<_> = n
                    .host_info
                    .as_ref()?
                    .system_info
                    .as_ref()?
                    .pods
                    .as_ref()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter(|p| p.filter_names(&name, &namespace))
                    .map(|pod| PodListItem::from(pod))
                    .collect();
                Some(pods)
            })
            .flatten()
            .collect()
    }
}
