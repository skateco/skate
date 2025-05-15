use crate::get::lister::{Lister, NameFilters};
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::{PodParent, PodmanPodInfo, PodmanPodStatus};
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
    #[serde(skip)]
    pub namespace: String,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub desired: String,
    #[serde(skip)]
    pub current: String,
    #[serde(skip)]
    pub ready: String,
    #[serde(skip)]
    pub up_to_date: String,
    #[serde(skip)]
    pub available: String,
    #[serde(skip)]
    pub node_selector: String,
    #[serde(skip)]
    pub age: String,
    #[tabled(skip)]
    #[serde(flatten)]
    pub manifest: serde_yaml::Value,
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
        let id = args.id.clone().unwrap_or_default();
        let ns = args.namespace.clone().unwrap_or_default();
        let daemonsets = state.catalogue(None, &[ResourceType::DaemonSet]);
        let daemonsets = daemonsets
            .into_iter()
            .filter(|d| d.object.matches_ns_name(&id, &ns))
            .collect::<Vec<_>>();

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
                        let pod_daemonset = p.daemonset();
                        if pod_daemonset.is_empty() {
                            return None;
                        }

                        if p.matches_parent_ns_name(PodParent::Deployment, &id, &ns) {
                            return Some((
                                state.nodes.len(),
                                format!("{}.{}", pod_daemonset, p.namespace()),
                                p,
                            ));
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

        let daemonset_pods = pods.fold(
            HashMap::<String, Vec<PodmanPodInfo>>::new(),
            |mut acc, (_num_nodes, depl, pod)| {
                acc.entry(depl).or_default().push(pod);
                acc
            },
        );

        daemonsets
            .into_iter()
            .map(|d| {
                let fallback = vec![];

                let all_pods = daemonset_pods
                    .get(&d.object.name.to_string())
                    .unwrap_or(&fallback);

                let health_pods = all_pods
                    .iter()
                    .filter(|p| PodmanPodStatus::Running == p.status)
                    .collect_vec()
                    .len();

                let created = all_pods.iter().fold(Local::now(), |acc, item| {
                    if item.created < acc {
                        return item.created;
                    }
                    acc
                });

                let node_selector = d
                    .object
                    .manifest
                    .as_ref()
                    .unwrap_or(&serde_yaml::Value::Null)["spec"]["selector"]["nodeSelector"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                DaemonsetListItem {
                    namespace: d.object.namespace(),
                    name: d.object.name(),
                    // TODO - not true, depends on selectors I'd guess
                    desired: state.nodes.len().to_string(),
                    current: all_pods.len().to_string(),
                    ready: health_pods.to_string(),
                    // TODO
                    up_to_date: "".to_string(),
                    // TODO
                    available: "".to_string(),
                    node_selector,
                    age: age(created),
                    manifest: d.object.manifest.clone().unwrap_or(serde_yaml::Value::Null),
                }
            })
            .collect()
    }
}
