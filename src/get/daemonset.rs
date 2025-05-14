use crate::get::lister::{Lister, NameFilters};
use crate::get::{GetObjectArgs, ResourceLister};
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::age;
use chrono::Local;
use itertools::Itertools;
use serde::Serialize;
use std::collections::HashMap;
use tabled::Tabled;

pub(crate) struct DaemonsetLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct DaemonsetListItem {
    pub namespace: String,
    pub name: String,
    pub desired: String,
    pub current: String,
    pub ready: String,
    pub up_to_date: String,
    pub available: String,
    pub node_selector: String,
    pub age: String,
}

impl NameFilters for DaemonsetListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

impl Lister<DaemonsetListItem> for DaemonsetLister {
    fn list(
        &self,
        _: ResourceType,
        args: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<DaemonsetListItem> {
        let pods = state
            .nodes
            .iter()
            .filter_map(|n| {
                let items: Vec<_> = n
                    .host_info
                    .clone()?
                    .system_info?
                    .pods
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|p| {
                        let daemonset = p
                            .labels
                            .get("skate.io/daemonset")
                            .unwrap_or(&"".to_string())
                            .clone();
                        if daemonset.is_empty() {
                            return None;
                        }

                        let res = {
                            let pref = &p;
                            pref.filter_names(
                                &args.id.clone().unwrap_or_default(),
                                &args.namespace.clone().unwrap_or_default(),
                            )
                        };
                        if res {
                            return Some((state.nodes.len(), daemonset, p));
                        }
                        None
                    })
                    .collect();
                match items.len() {
                    0 => None,
                    _ => Some(items),
                }
            })
            .flatten();

        let pods = pods.fold(
            HashMap::<String, Vec<PodmanPodInfo>>::new(),
            |mut acc, (_num_nodes, depl, pod)| {
                acc.entry(depl).or_default().push(pod);
                acc
            },
        );

        pods.iter()
            .map(|(n, pods)| {
                let health_pods = pods
                    .iter()
                    .filter(|p| PodmanPodStatus::Running == p.status)
                    .collect_vec()
                    .len();
                let _all_pods = pods.len();
                let created = pods.iter().fold(Local::now(), |acc, item| {
                    if item.created < acc {
                        return item.created;
                    }
                    acc
                });
                let namespace = pods
                    .first()
                    .unwrap()
                    .labels
                    .get("skate.io/namespace")
                    .unwrap_or(&"default".to_string())
                    .clone();
                let node_selector = pods
                    .first()
                    .unwrap()
                    .labels
                    .iter()
                    .filter(|(k, _)| k.starts_with("nodeselector/"))
                    .map(|(k, _v)| k.clone())
                    .collect_vec()
                    .join(",");

                DaemonsetListItem {
                    namespace,
                    name: n.clone(),
                    desired: state.nodes.len().to_string(),
                    current: pods.len().to_string(),
                    ready: health_pods.to_string(),
                    up_to_date: "".to_string(),
                    available: "".to_string(),
                    node_selector,
                    age: age(created),
                }
            })
            .collect()
    }
}
