use std::collections::HashMap;
use chrono::Local;
use itertools::Itertools;
use crate::get::{GetObjectArgs, Lister};
use crate::skatelet::SystemInfo;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::{age, NamespacedName};

pub(crate) struct DeploymentLister {}

impl Lister<(NamespacedName, PodmanPodInfo)> for DeploymentLister {
    fn selector(&self, _si: &SystemInfo, _ns: &str, _id: &str) -> Option<Vec<(NamespacedName, PodmanPodInfo)>> {
        todo!()
    }
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<(NamespacedName, PodmanPodInfo)> {
        let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
            let items: Vec<_> = n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().filter_map(|p| {
                let ns = args.namespace.clone();
                let id = args.id.clone();
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
                            return Some((NamespacedName::from(format!("{}.{}", deployment, pod_ns).as_str()), p));
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

    fn print(&self, items: Vec<(NamespacedName, PodmanPodInfo)>) {
        println!(
            "{0: <30}  {1: <30}  {2: <10}  {3: <10}  {4: <10}  {5: <30}",
            "NAMESPACE","NAME", "READY", "UP-TO-DATE", "AVAILABLE", "AGE"
        );
        let pods = items.into_iter().fold(HashMap::<NamespacedName, Vec<PodmanPodInfo>>::new(), |mut acc, (depl, pod)| {
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
                "{0: <30}  {1: <30}  {2: <10}  {3: <10}  {4: <10}  {5: <30}",
                deployment.namespace, deployment.name, format!("{}/{}", health_pods, all_pods), all_pods, health_pods, age(created)
            )
        }
    }
}
