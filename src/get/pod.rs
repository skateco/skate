use tabled::Tabled;
use crate::get::Lister;
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::skatelet::system::podman::PodmanPodInfo;
use crate::util::age;

pub(crate) struct PodLister {}

impl NameFilters for &PodmanPodInfo {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn name(&self) -> String {
        self.labels.get("skate.io/name").cloned().unwrap_or("".to_string())
    }

    fn namespace(&self) -> String {
        self.labels.get("skate.io/namespace").cloned().unwrap_or("".to_string())
    }
}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct PodListItem {
    pub namespace: String,
    pub name: String,
    pub ready: String,
    pub status: String,
    pub restarts: String,
    pub age: String,
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
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<PodListItem> {
        si.pods.as_ref().unwrap_or(&vec!()).iter().filter(|p| {
            let filterable: Box<dyn NameFilters> = Box::new(*p);
            filterable.filter_names(id, ns)
        }).map(|pod| {
            let num_containers = pod.containers.clone().unwrap_or_default().len();
            let healthy_containers = pod.containers.clone().unwrap_or_default().iter().filter(|c| {
                match c.status.as_str() {
                    "running" => true,
                    _ => false
                }
            }).collect::<Vec<_>>().len();
            let restarts = pod.containers.clone().unwrap_or_default().iter().map(|c| c.restart_count.unwrap_or_default())
                .reduce(|a, c| a + c).unwrap_or_default();

            PodListItem {
                namespace: pod.namespace(),
                name: pod.name(),
                ready: format!("{}/{}", healthy_containers, num_containers),
                status: pod.status.to_string(),
                restarts: restarts.to_string(),
                age: age(pod.created),
            }
        }).collect()
    }
}
