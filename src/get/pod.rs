use chrono::SecondsFormat;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::{PodmanPodInfo, SystemInfo};
use crate::util::age;

pub (crate) struct PodLister {}

impl NameFilters for &PodmanPodInfo {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn name(&self) -> String {
        self.labels.get("skate.io/name").map(|ns| ns.clone()).unwrap_or("".to_string())
    }

    fn namespace(&self) -> String {
        self.labels.get("skate.io/namespace").map(|ns| ns.clone()).unwrap_or("".to_string())
    }
}


impl Lister<PodmanPodInfo> for PodLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<PodmanPodInfo>> {
        return si.pods.as_ref().and_then(|pods| {
            Some(pods.iter().filter(|p| {
                let filterable: Box<dyn NameFilters> = Box::new(*p);
                filterable.filter_names(id, ns)
            }).map(|p| p.clone()).collect())
        })
    }

    fn print(&self, pods: Vec<PodmanPodInfo>) {
        println!(
            "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
            "NAME", "READY", "STATUS", "RESTARTS", "AGE"
        );
        for pod in pods {
            let num_containers = pod.containers.clone().unwrap_or_default().len();
            let healthy_containers = pod.containers.clone().unwrap_or_default().iter().filter(|c| {
                match c.status.as_str() {
                    "running" => true,
                    _ => false
                }
            }).collect::<Vec<_>>().len();
            let restarts = pod.containers.clone().unwrap_or_default().iter().map(|c| c.restart_count.unwrap_or_default())
                .reduce(|a, c| a + c).unwrap_or_default();
            println!(
                "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
                pod.name, format!("{}/{}", healthy_containers, num_containers), pod.status, restarts, age(pod.created)
            )
        }
    }
}
