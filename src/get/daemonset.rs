use std::collections::HashMap;
use chrono::Local;
use itertools::Itertools;
use tabled::builder::Builder;
use tabled::settings::Style;
use tabled::Tabled;
use crate::get::{GetObjectArgs, Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::{age, NamespacedName};

pub(crate) struct DaemonsetLister {}

#[derive(Tabled)]
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

impl Lister<DaemonsetListItem> for DaemonsetLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Vec<DaemonsetListItem> {
        todo!()
    }
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<DaemonsetListItem> {
        let pods = state.nodes.iter().filter_map(|n| {
            let items: Vec<_> = n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().filter_map(|p| {
                let daemonset = p.labels.get("skate.io/daemonset").unwrap_or(&"".to_string()).clone();
                if daemonset == "" {
                    return None;
                }

                if {
                    let filterable: Box<dyn NameFilters> = Box::new(&p);
                    filterable.filter_names(&args.id.clone().unwrap_or_default(), &args.namespace.clone().unwrap_or_default())
                } {
                    return Some((state.nodes.len(), daemonset, p));
                }
                None
            }).collect();
            match items.len() {
                0 => None,
                _ => Some(items)
            }
        }).flatten();

        let pods = pods.fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (_num_nodes, depl, pod)| {
            acc.entry(depl).or_insert(vec![]).push(pod);
            acc
        });

        pods.iter().map(|(n, pods)| {
            let health_pods = pods.iter().filter(|p| PodmanPodStatus::Running == p.status).collect_vec().len();
            let _all_pods = pods.len();
            let created = pods.iter().fold(Local::now(), |acc, item| {
                if item.created < acc {
                    return item.created;
                }
                return acc;
            });
            let namespace = pods.first().unwrap().labels.get("skate.io/namespace").unwrap_or(&"default".to_string()).clone();
            let node_selector = pods.first().unwrap().labels.iter().filter(|(k, _)| k.starts_with("nodeselector/")).map(|(k, _v)| k.clone()).collect_vec().join(",");
            let item = DaemonsetListItem {
                namespace,
                name: n.clone(),
                desired: n.to_string(),
                current: pods.len().to_string(),
                ready: health_pods.to_string(),
                up_to_date: "".to_string(),
                available: "".to_string(),
                node_selector,
                age: age(created),
            };
            item
        }).collect()
    }

    // fn print(&self, items: Vec<(usize, String, PodmanPodInfo)>) {
    //     let mut rows = vec!(["NAMESPACE", "NAME", "DESIRED", "CURRENT", "READY", "UP-TO-DATE", "AVAILABLE", "NODE SELECTOR", "AGE"].map(|i| i.to_string()));
    //     let num_nodes = items.first().unwrap().0;
    //     let pods = items.into_iter().fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (_num_nodes, depl, pod)| {
    //         acc.entry(depl).or_insert(vec![]).push(pod);
    //         acc
    //     });
    //
    //     for (name, pods) in pods {
    //         let health_pods = pods.iter().filter(|p| PodmanPodStatus::Running == p.status).collect_vec().len();
    //         let _all_pods = pods.len();
    //         let created = pods.iter().fold(Local::now(), |acc, item| {
    //             if item.created < acc {
    //                 return item.created;
    //             }
    //             return acc;
    //         });
    //         let namespace = pods.first().unwrap().labels.get("skate.io/namespace").unwrap_or(&"default".to_string()).clone();
    //         let node_selector = pods.first().unwrap().labels.iter().filter(|(k, _)| k.starts_with("nodeselector/")).map(|(k, _v)| k.clone()).collect_vec().join(",");
    //
    //         // assuming that we want same number as nodes, that's wrong but anyway
    //             rows.push([namespace, name, num_nodes.to_string(), pods.len().to_string(), health_pods.to_string(), "".to_string(), "".to_string(), node_selector, age(created)]);
    //     }
    //
    //     let mut table = Builder::from_iter(rows).build();
    //     table.with(Style::empty());
    //     println!("{}", table);
    // }
}
