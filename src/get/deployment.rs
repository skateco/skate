use std::collections::HashMap;
use chrono::{Local, SecondsFormat};
use itertools::Itertools;
use crate::get::{GetObjectArgs, IdCommand, Lister};
use crate::skatelet::{PodmanPodInfo, PodmanPodStatus, SystemInfo};
use crate::state::state::ClusterState;

pub (crate) struct DeploymentLister {}

impl Lister<(String, PodmanPodInfo)> for DeploymentLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<(String, PodmanPodInfo)>> {
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
                match deployment {
                    Some(deployment) => {
                        let match_ns = match ns.clone() {
                            Some(ns) => {
                                ns == p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone()
                            }
                            None => false
                        };
                        let match_id = match id.clone() {
                            Some(id) => {
                                id == deployment.clone()
                            }
                            None => false
                        };
                        if match_ns || match_id || (id.is_none() && ns.is_none()) {
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
            "NAME", "READY", "STATUS", "RESTARTS", "CREATED"
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
                deployment, format!("{}/{}", health_pods, all_pods), "", "", created.to_rfc3339_opts(SecondsFormat::Secs, true)
            )
        }
    }
}
