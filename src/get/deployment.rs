use std::collections::HashMap;
use chrono::Local;
use itertools::Itertools;
use crate::get::{GetObjectArgs, IdCommand, Lister};
use crate::skatelet::SystemInfo;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::age;

pub(crate) struct DeploymentLister {}

impl Lister<(String, PodmanPodInfo)> for DeploymentLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Option<Vec<(String, PodmanPodInfo)>> {
        todo!()
    }
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<(String, PodmanPodInfo)> {
        let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
            let items: Vec<_> = n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().filter_map(|p| {
                let ns = args.namespace.clone();
                let id = match args.id.clone() {
                    Some(cmd) => match cmd {
                        IdCommand::Id(ids) => Some(ids.into_iter().next().unwrap_or("".to_string()))
                    }
                    None => None
                };
                let deployment = p.labels.get("skate.io/deployment");
                let pod_ns = p.labels.get("skate.io/namespace").unwrap_or(&"default".to_string()).clone();
                match deployment {
                    Some(deployment) => {
                        let match_ns = match ns.clone() {
                            Some(ns) => {
                                ns == pod_ns
                            }
                            None => false
                        };
                        let match_id = match id.clone() {
                            Some(id) => {
                                id == deployment.clone()
                            }
                            None => false
                        };
                        if match_ns || match_id || (id.is_none() && ns.is_none() && pod_ns != "skate") {
                            return Some((deployment.clone(), p));
                        }
                        None
                    }
                    None => None
                }
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
