use std::collections::HashMap;
use chrono::{Local};
use itertools::Itertools;
use crate::get::{GetObjectArgs, IdCommand, Lister};
use crate::skatelet::{PodmanPodInfo, PodmanPodStatus, SystemInfo};
use crate::state::state::ClusterState;
use crate::util::age;

pub(crate) struct DaemonsetLister {}

impl Lister<(String, PodmanPodInfo)> for DaemonsetLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Option<Vec<(String, PodmanPodInfo)>> {
        todo!()
    }
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<(String, PodmanPodInfo)> {
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
                    return Some((daemonset, p));
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

    fn print(&self, items: Vec<(String, PodmanPodInfo)>) {
        println!(
            "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
            "NAME", "READY", "STATUS", "RESTARTS", "AGE"
        );
        let pods = items.into_iter().fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (depl, pod)| {
            acc.entry(depl).or_insert(vec![]).push(pod);
            acc
        });

        for (deployment, pods) in pods {
            let health_pods = pods.iter().filter(|p| PodmanPodStatus::Running == p.status).collect_vec().len();
            let all_pods = pods.len();
            let created = pods.iter().fold(Local::now(), |acc, item| {
                if item.created < acc {
                    return item.created;
                }
                return acc;
            });

            println!(
                "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
                deployment, format!("{}/{}", health_pods, all_pods), "", "", age(created)
            )
        }
    }
}
