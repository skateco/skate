use crate::get::GetObjectArgs;
use crate::get::lister::{Lister, NameFilters};
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::{PodParent, PodmanPodInfo, PodmanPodStatus};
use crate::state::state::ClusterState;
use crate::util::{NamespacedName, SkateLabels, age, get_skate_label_value};
use chrono::Local;
use serde::Serialize;
use std::collections::HashMap;
use tabled::Tabled;

pub(crate) struct DeploymentLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct DeploymentListItem {
    #[serde(skip)]
    pub namespace: String,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub ready: String,
    #[serde(skip)]
    pub up_to_date: String,
    #[serde(skip)]
    pub available: String,
    #[serde(skip)]
    pub age: String,
    #[tabled(skip)]
    #[serde(flatten)]
    pub manifest: serde_yaml::Value,
}

impl NameFilters for DeploymentListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

impl Lister<DeploymentListItem> for DeploymentLister {
    fn list(
        &self,
        _: ResourceType,
        args: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<DeploymentListItem> {
        let id = args.id.clone().unwrap_or_default();
        let ns = args.namespace.clone().unwrap_or_default();
        let deployments = state.catalogue(None, &[ResourceType::Deployment], Some(&ns), Some(&id));

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
                        let pod_deployment = p.deployment();
                        if pod_deployment.is_empty() {
                            return None;
                        }

                        if p.matches_parent_ns_name(PodParent::Deployment, &id, &ns) {
                            let pod_ns = get_skate_label_value(
                                &Some(p.labels.clone()),
                                &SkateLabels::Namespace,
                            )
                            .unwrap_or("default".to_string());

                            return Some((
                                NamespacedName::from(
                                    format!("{}.{}", pod_deployment, pod_ns).as_str(),
                                ),
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

        let deployment_pods = pods.fold(
            HashMap::<NamespacedName, Vec<PodmanPodInfo>>::new(),
            |mut acc, (depl, pod)| {
                acc.entry(depl).or_default().push(pod);
                acc
            },
        );

        deployments
            .into_iter()
            .map(|d| {
                let fallback = vec![];
                let all_pods = deployment_pods.get(&d.object.name).unwrap_or(&fallback);

                let health_pods = all_pods
                    .iter()
                    .filter(|p| PodmanPodStatus::Running == p.status)
                    .count();

                let created = all_pods.iter().fold(Local::now(), |acc, item| {
                    if item.created < acc {
                        return item.created;
                    }
                    acc
                });

                let its_age = age(created);
                let healthy = format!("{}/{}", health_pods, all_pods.len());
                DeploymentListItem {
                    namespace: d.object.name.namespace.clone(),
                    name: d.object.name.name.clone(),
                    ready: healthy,
                    up_to_date: all_pods.len().to_string(),
                    available: health_pods.to_string(),
                    age: its_age,
                    manifest: d.object.manifest.clone().unwrap_or(serde_yaml::Value::Null),
                }
            })
            .collect()
    }
}
