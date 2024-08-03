use std::collections::HashMap;
use chrono::Local;
use itertools::Itertools;
use crate::get::{GetObjectArgs, IdCommand, Lister};
use crate::skatelet::SystemInfo;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::age;

pub(crate) struct DaemonsetLister {}

impl Lister<(usize, String, PodmanPodInfo)> for DaemonsetLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Option<Vec<(usize, String, PodmanPodInfo)>> {
        todo!()
    }
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<(usize, String, PodmanPodInfo)> {
        let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
            let items: Vec<_> = n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().filter_map(|p| {
                let ns = args.namespace.clone().unwrap_or("default".to_string());
                let id = match args.id.clone() {
                    Some(cmd) => match cmd {
                        IdCommand::Id(ids) => Some(ids.into_iter().next().unwrap_or("".to_string()))
                    }
                    None => None
                };
                let daemonset = p.labels.get("skate.io/daemonset").and_then(|n| Some(n.clone())).unwrap_or_default();
                if daemonset == "" {
                    return None;
                }
                let daemonset_ns = p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone();


                let match_ns = ns == daemonset_ns;

                let match_id = match id.clone() {
                    Some(id) => {
                        id == daemonset
                    }
                    None => false
                };
                if match_ns || match_id || (id.is_none() && ns == "" && daemonset_ns != "skate" ) {
                    return Some((state.nodes.len(), daemonset, p));
                }
                None
            }).collect();
            match items.len() {
                0 => None,
                _ => Some(items)
            }
        }).flatten().collect();
        pods
    }

    fn print(&self, items: Vec<(usize, String, PodmanPodInfo)>) {
        // NAMESPACE     NAME         DESIRED   CURRENT   READY   UP-TO-DATE   AVAILABLE   NODE SELECTOR            AGE
        macro_rules! cols {
            () => ("{0: <15}  {1: <15}  {2: <12}  {3: <12}  {4: <12}  {5: <12}  {6: <12}  {7: <50}  {8: <15}")
        }
        println!(
            cols!(),
            "NAMESPACE", "NAME", "DESIRED", "CURRENT", "READY", "UP-TO-DATE", "AVAILABLE", "NODE SELECTOR", "AGE"
        );
        let num_nodes = items.first().unwrap().0;
        let pods = items.into_iter().fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (num_nodes, depl, pod)| {
            acc.entry(depl).or_insert(vec![]).push(pod);
            acc
        });

        for (name, pods) in pods {
            let health_pods = pods.iter().filter(|p| PodmanPodStatus::Running == p.status).collect_vec().len();
            let all_pods = pods.len();
            let created = pods.iter().fold(Local::now(), |acc, item| {
                if item.created < acc {
                    return item.created;
                }
                return acc;
            });
            let namespace = pods.first().unwrap().labels.get("skate.io/namespace").unwrap_or(&"default".to_string()).clone();
            let node_selector = pods.first().unwrap().labels.iter().filter(|(k, _)| k.starts_with("nodeselector/")).map(|(k, v)| format!("{}={}", k, v)).collect_vec().join(",");

            // assuming that we want same number as nodes, that's wrong but anyway
            println!(
                cols!(),
                namespace, name, num_nodes, pods.len(), health_pods, "", "", node_selector, age(created)
            )
        }
    }
}
