use crate::config::{cache_dir, Config};
use crate::filestore::ObjectListItem;
use crate::get::lister::NameFilters;
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::{
    Node as K8sNode, NodeAddress, NodeSpec, NodeStatus as K8sNodeStatus,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::Display;
use std::fs::File;
use std::path::Path;
use strum_macros::Display;
use tabled::Tabled;

use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::system::podman::{PodmanPodInfo, PodmanPodStatus};
use crate::skatelet::SystemInfo;
use crate::ssh::HostInfo;
use crate::state::state::NodeStatus::{Healthy, Unhealthy, Unknown};
use crate::supported_resources::SupportedResources;
use crate::util::{metadata_name, slugify, tabled_display_option, SkateLabels};

#[derive(Serialize, Deserialize, Clone, Debug, Display, PartialEq, Default)]
pub enum NodeStatus {
    #[default]
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Tabled, Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeState {
    pub node_name: String,
    pub status: NodeStatus,
    #[tabled(display("tabled_display_option"))]
    pub message: Option<String>,
    #[tabled(skip)]
    pub host_info: Option<HostInfo>,
}

impl From<&NodeState> for K8sNode {
    fn from(val: &NodeState) -> Self {
        let mut metadata = ObjectMeta::default();
        let mut spec = NodeSpec::default();
        let mut status = K8sNodeStatus::default();

        metadata.name = Some(val.node_name.clone());
        metadata.namespace = Some("default".to_string());
        metadata.uid = Some(val.node_name.clone());

        status.phase = match val.status {
            Unknown => Some("Pending".to_string()),
            Healthy => Some("Ready".to_string()),
            Unhealthy => Some("Pending".to_string()),
        };

        spec.unschedulable = Some(!val.schedulable());

        let sys_info = val.host_info.as_ref().and_then(|h| h.system_info.clone());

        (
            status.capacity,
            status.allocatable,
            status.addresses,
            metadata.labels,
        ) = match sys_info {
            Some(si) => (
                Some(BTreeMap::<String, Quantity>::from([
                    ("cpu".to_string(), Quantity(format!("{}", si.num_cpus))),
                    (
                        "memory".to_string(),
                        Quantity(format!("{} Mib", si.total_memory_mib)),
                    ),
                ])),
                (Some(BTreeMap::<String, Quantity>::from([
                    (
                        "cpu".to_string(),
                        Quantity(format!(
                            "{}",
                            (si.num_cpus as f32) * (100.00 - si.cpu_usage) / 100.0
                        )),
                    ),
                    (
                        "memory".to_string(),
                        Quantity(format!("{} Mib", si.total_memory_mib - si.used_memory_mib)),
                    ),
                ]))),
                ({
                    let mut addresses = vec![NodeAddress {
                        address: si.hostname.clone(),
                        type_: "Hostname".to_string(),
                    }];
                    if let Some(ip) = si.internal_ip_address {
                        addresses.push(NodeAddress {
                            address: ip,
                            type_: "InternalIP".to_string(),
                        })
                    }
                    Some(addresses)
                }),
                Some(BTreeMap::<String, String>::from([
                    (SkateLabels::Arch.to_string(), si.platform.arch.clone()),
                    (SkateLabels::Nodename.to_string(), val.node_name.clone()),
                    (SkateLabels::Hostname.to_string(), si.hostname.clone()),
                ])),
            ),
            None => (None, None, None, None),
        };

        K8sNode {
            metadata,
            spec: Some(spec),
            status: Some(status),
        }
    }
}

impl NodeState {
    /// scores the node for scheduling based on free memory and cpu
    pub fn system_info(&self) -> Option<&SystemInfo> {
        self.host_info.as_ref().and_then(|h| h.system_info.as_ref())
    }
    pub fn filter_pods(&self, f: &dyn Fn(&PodmanPodInfo) -> bool) -> Vec<PodmanPodInfo> {
        self.host_info
            .as_ref()
            .and_then(|h| {
                h.system_info.clone().and_then(|i| {
                    i.pods
                        .map(|p| p.clone().into_iter().filter(|p| f(p)).collect::<Vec<_>>())
                })
            })
            .unwrap_or_default()
    }

    pub fn reconcile_object_creation(
        &mut self,
        object: &SupportedResources,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => {
                self.reconcile_pod_creation(&PodmanPodInfo::from((*pod).clone()))
            }
            // This is a no-op since the only thing that happens when during the Deployment's ScheduledOperation is that we write the manifest to file for future reference
            // The state change is all done by the Pods' scheduled operations
            SupportedResources::Deployment(_) => {
                /* nothing to do */
                Ok(ReconciledResult::default())
            }
            SupportedResources::DaemonSet(_) => {
                /* nothing to do */
                Ok(ReconciledResult::default())
            }
            _ => self.reconcile_resource_creation(object),
        }
    }

    pub fn reconcile_object_deletion(
        &mut self,
        object: &SupportedResources,
    ) -> Result<crate::state::state::ReconciledResult, Box<dyn Error>> {
        match object {
            SupportedResources::Pod(pod) => {
                self.reconcile_pod_deletion(&PodmanPodInfo::from((*pod).clone()))
            }
            SupportedResources::Deployment(deployment) => {
                self.reconcile_deployment_deletion(deployment)
            }
            SupportedResources::DaemonSet(daemonset) => {
                self.reconcile_daemonset_deletion(daemonset)
            }
            _ => self.reconcile_resource_deletion(object),
        }
    }

    fn reconcile_resource_deletion(
        &mut self,
        object: &SupportedResources,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                Some(
                    si.resources
                        .iter()
                        .filter(|r| r.resource_type == object.into())
                        .collect::<Vec<_>>(),
                )
            })
        });
        Ok(ReconciledResult::removed())
    }

    fn reconcile_resource_creation(
        &mut self,
        object: &SupportedResources,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                if let Some(resource) = object.try_into().ok() {
                    si.resources.push(resource)
                }
                Some(si)
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_pod_creation(
        &mut self,
        pod: &PodmanPodInfo,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().map(|pods| {
                    let mut pod = pod.clone();
                    pod.status = PodmanPodStatus::Created;
                    pods.push(pod);
                })
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_pod_deletion(
        &mut self,
        pod: &PodmanPodInfo,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods
                    .as_mut()
                    .map(|pods| pods.retain(|p| p.name != pod.name))
            })
        });

        Ok(ReconciledResult::removed())
    }

    fn reconcile_daemonset_deletion(
        &mut self,
        daemonset: &DaemonSet,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        let name = metadata_name(daemonset).to_string();
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().map(|pods| {
                    pods.iter()
                        .filter(|p| !p.daemonset().is_empty() && p.daemonset() != name)
                })
            })
        });
        Ok(ReconciledResult::removed())
    }

    fn reconcile_deployment_deletion(
        &mut self,
        deployment: &Deployment,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        let name = metadata_name(deployment).to_string();
        self.host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().map(|pods| {
                    pods.iter()
                        .filter(|p| !p.deployment().is_empty() && p.deployment() != name)
                })
            })
        });
        Ok(ReconciledResult::removed())
    }

    // whether we can schedule workloads on this node
    pub fn schedulable(&self) -> bool {
        if self.status != Healthy {
            return false;
        }

        self.host_info
            .as_ref()
            .and_then(|hi| hi.system_info.as_ref().map(|si| !si.cordoned))
            .unwrap_or(true)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ClusterState {
    pub cluster_name: String,
    pub nodes: Vec<NodeState>,
}

#[derive(Default)]
pub struct ReconciledResult {
    pub removed: usize,
    pub added: usize,
    pub updated: usize,
}
impl ReconciledResult {
    fn removed() -> ReconciledResult {
        ReconciledResult {
            removed: 1,
            added: 0,
            updated: 0,
        }
    }
    fn added() -> ReconciledResult {
        ReconciledResult {
            removed: 0,
            added: 1,
            updated: 0,
        }
    }
    fn updated() -> ReconciledResult {
        ReconciledResult {
            removed: 0,
            added: 0,
            updated: 1,
        }
    }
}

/// Unwraps an Option, returning the value if it is Some, or continuing the loop if it is None.
macro_rules! unwrap_or_continue {
    ($opt: expr) => {
        match $opt {
            Some(v) => v,
            None => {
                continue;
            }
        }
    };
}

impl ClusterState {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    #[allow(unused)]
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(
            ClusterState::path(&self.cluster_name.clone()).as_str(),
        ))
        .map_err(|e| anyhow!("failed to open or create state file").context(e))?;
        serde_json::to_writer(state_file, self)
            .map_err(|e| anyhow!("failed to serialize state").context(e))?;
        Ok(())
    }

    pub fn load(cluster_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::create(ClusterState::path(cluster_name))
            .map_err(|e| anyhow!("failed to open or create state file").context(e))?;

        let result = serde_json::from_reader::<_, ClusterState>(file)
            .map_err(|e| anyhow!("failed to parse cluster state").context(e));

        match result {
            Ok(state) => Ok(state),
            Err(_e) => {
                let state = ClusterState {
                    cluster_name: cluster_name.to_string(),
                    nodes: vec![],
                };
                Ok(state)
            }
        }
    }

    #[allow(unused)]
    pub fn reconcile_node(&mut self, node: &HostInfo) -> Result<ReconciledResult, Box<dyn Error>> {
        let pos = self
            .nodes
            .iter_mut()
            .find_position(|n| n.node_name == node.node_name);

        let result = match pos {
            Some((p, _obj)) => {
                self.nodes[p] = (*node).clone().into();
                ReconciledResult::updated()
            }
            None => {
                self.nodes.push((*node).clone().into());
                ReconciledResult::added()
            }
        };

        Ok(result)
    }

    pub fn reconcile_object_creation(
        &mut self,
        object: &SupportedResources,
        node_name: &str,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        let node = self
            .nodes
            .iter_mut()
            .find(|n| n.node_name == node_name)
            .ok_or(anyhow!("node not found: {}", node_name))?;
        node.reconcile_object_creation(object)
    }

    pub fn reconcile_object_deletion(
        &mut self,
        object: &SupportedResources,
        node_name: &str,
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        let node = self
            .nodes
            .iter_mut()
            .find(|n| n.node_name == node_name)
            .ok_or(anyhow!("node not found: {}", node_name))?;
        node.reconcile_object_deletion(object)
    }

    pub fn reconcile_all_nodes(
        &mut self,
        cluster_name: &str,
        config: &Config,
        host_info: &[HostInfo],
    ) -> Result<ReconciledResult, Box<dyn Error>> {
        let cluster = config.active_cluster(Some(cluster_name.to_string()))?;

        let state_hosts: HashSet<String> = self.nodes.iter().map(|n| n.node_name.clone()).collect();

        let config_hosts: HashSet<String> = cluster.nodes.iter().map(|n| n.name.clone()).collect();

        let new = &config_hosts - &state_hosts;
        let orphaned = &state_hosts - &config_hosts;

        self.nodes = self
            .nodes
            .iter()
            .filter_map(|n| match orphaned.contains(&n.node_name) {
                false => Some(n.clone()),
                true => None,
            })
            .collect();

        let mut new_nodes: Vec<NodeState> = config
            .active_cluster(Some(cluster_name.to_string()))?
            .nodes
            .iter()
            .filter_map(|n| match new.contains(&n.name) {
                true => Some(NodeState {
                    node_name: n.name.clone(),
                    status: Unknown,
                    message: None,
                    host_info: None,
                }),
                false => None,
            })
            .collect();

        self.nodes.append(&mut new_nodes);

        let mut updated = 0;
        // now that we have our list, go through and mark them healthy or unhealthy
        self.nodes = self
            .nodes
            .iter()
            .map(|node| {
                let mut node = node.clone();
                match host_info.iter().find(|h| h.node_name == node.node_name) {
                    Some(info) => {
                        updated += 1;
                        (node.status, node.message) = match info.healthy() {
                            Ok(_) => (Healthy, None),
                            Err(errs) => {
                                let err_string = errs.join(". ").to_string();
                                (Unhealthy, Some(err_string))
                            }
                        };
                        node.host_info = Some(info.clone())
                    }
                    None => {
                        node.status = Unknown;
                    }
                };
                node
            })
            .collect();

        Ok(ReconciledResult {
            removed: orphaned.len(),
            added: new.len(),
            updated,
        })
    }

    pub fn filter_pods(
        &self,
        f: &dyn Fn(&PodmanPodInfo) -> bool,
    ) -> Vec<(PodmanPodInfo, &NodeState)> {
        let res: Vec<_> = self
            .nodes
            .iter()
            .flat_map(|n| {
                n.filter_pods(&|p| f(p))
                    .into_iter()
                    .map(|p| (p, n))
                    .collect::<Vec<_>>()
            })
            .collect();
        res
    }

    pub fn locate_pods(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        // the pod manifest name is what podman will use to name the pod, thus has the namespace in it as a suffix
        self.filter_pods(&|p| {
            p.name == format!("{}.{}", name, namespace) && p.namespace() == namespace
        })
    }

    pub fn locate_deployment_pods(
        &self,
        name: &str,
        namespace: &str,
    ) -> Vec<(PodmanPodInfo, &NodeState)> {
        let name = name
            .strip_prefix(format!("{}.", namespace).as_str())
            .unwrap_or(name);
        self.filter_pods(&|p| p.deployment() == name && p.namespace() == namespace)
    }

    // the catalogue is the list of 'applied' resources.
    // does not include pods created due to another resource being applied
    pub fn catalogue_mut(
        &mut self,
        filter_node: Option<&str>,
        filter_types: &[ResourceType],
    ) -> Vec<MutCatalogueItem> {
        self.nodes
            .iter_mut()
            .filter(|n| filter_node.is_none() || n.node_name == filter_node.unwrap())
            .filter_map(|n| {
                n.host_info.as_mut().and_then(|hi| {
                    hi.system_info
                        .as_mut()
                        .map(|si| extract_mut_catalog(&n.node_name, si, filter_types))
                })
            })
            .flatten()
            // sort by time descending
            .sorted_by(|a, b| a.object.updated_at.cmp(&b.object.updated_at))
            // will ignore duplicates,
            .unique_by(|x| format!("{}-{}", x.object.resource_type, x.object.name))
            .collect()
    }

    pub fn catalogue(
        &self,
        filter_node: Option<&str>,
        filter_types: &[ResourceType],
        namespace: Option<&str>,
        name: Option<&str>,
    ) -> Vec<CatalogueItem> {
        let mut map: HashMap<String, CatalogueItem> = HashMap::new();

        for node in &self.nodes {
            if filter_node.is_some() && node.node_name != filter_node.unwrap_or_default() {
                continue;
            }

            let si = node
                .host_info
                .as_ref()
                .and_then(|hi| hi.system_info.as_ref());
            let si = unwrap_or_continue!(si);

            let objects = extract_catalog(&si, filter_types, namespace, name);

            for object in objects {
                let key = format!("{}-{}", object.resource_type, object.name);

                if let Some(existing) = map.get_mut(&key) {
                    if object.generation == existing.object.generation {
                        // not a conflict
                        // push onto nodes if not alerady there
                        if !existing.nodes.contains(&&node) {
                            existing.nodes.push(&node);
                        }
                        continue;
                    } else if object.generation > existing.object.generation {
                        // object is newer
                        // replace and push existing onto conflicts
                        let conflict = ConflictingResource {
                            object: existing.object,
                            nodes: existing.nodes.clone(),
                            reason: ConflictReason::LesserVersion,
                        };

                        eprintln!(
                            "WARNING: {}: resource on {} has generation {} when latest is {}",
                            object.name,
                            existing
                                .nodes
                                .iter()
                                .map(|i| i.node_name.to_string())
                                .join(", "),
                            existing.object.generation,
                            object.generation
                        );

                        map.insert(
                            key,
                            CatalogueItem {
                                object,
                                conflict: vec![conflict],
                                nodes: vec![&node],
                            },
                        );
                    } else if object.generation < existing.object.generation {
                        // object is old
                        // put it onto conflicts
                        if let Some(existing_conflict) = existing
                            .conflict
                            .iter_mut()
                            .find(|i| i.object.generation == object.generation)
                        {
                            // already exists
                            if !existing_conflict.nodes.contains(&&node) {
                                existing_conflict.nodes.push(&node);
                            }
                        } else {
                            // new conflict
                            let conflict = ConflictingResource {
                                object,
                                nodes: vec![&node],
                                reason: ConflictReason::LesserVersion,
                            };
                            existing.conflict.push(conflict);
                        }

                        eprintln!(
                            "WARNING: {}: resource on {} has generation {} when latest is {}",
                            object.name,
                            node.node_name,
                            object.generation,
                            existing.object.generation
                        );
                    }
                } else {
                    let item = CatalogueItem {
                        object,
                        conflict: vec![],
                        nodes: vec![&node],
                    };
                    map.insert(key, item);
                }
            }
        }

        map.into_iter().map(|(_, item)| item).collect()
    }
}

fn extract_mut_catalog<'a>(
    n: &str,
    si: &'a mut SystemInfo,
    filter_types: &[ResourceType],
) -> Vec<MutCatalogueItem<'a>> {
    let all_types = filter_types.is_empty();

    si.resources
        .iter_mut()
        .filter(|c| all_types || filter_types.contains(&c.resource_type))
        .map(|o| MutCatalogueItem {
            object: o,
            node: n.to_string(),
        })
        .collect()
}

fn extract_catalog<'a>(
    si: &'a SystemInfo,
    filter_types: &[ResourceType],
    namespace: Option<&str>,
    name: Option<&str>,
) -> Vec<&'a ObjectListItem> {
    let all_types = filter_types.is_empty();

    (&si.resources)
        .into_iter()
        .filter(|c| all_types || filter_types.contains(&c.resource_type))
        .filter(|c| c.matches_ns_name(name.unwrap_or_default(), namespace.unwrap_or_default()))
        .collect()
}

// holds references to a resource
pub struct MutCatalogueItem<'a> {
    pub object: &'a mut ObjectListItem,
    #[allow(unused)]
    pub node: String,
}

pub enum ConflictReason {
    LesserVersion,
    // HashMismatch, TODO
}

pub struct ConflictingResource<'a, 'b> {
    pub object: &'a ObjectListItem,
    pub nodes: Vec<&'b NodeState>,
    pub reason: ConflictReason,
}

impl Display for ConflictingResource<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} on {} has {}",
            self.object.name,
            self.nodes
                .iter()
                .map(|i| i.node_name.to_string())
                .join(", "),
            match self.reason {
                ConflictReason::LesserVersion =>
                    format!("lower generation ({}) than latest", self.object.generation),
                // ConflictReason::HashMismatch => "Hash Mismatch",
            }
        )
    }
}
pub struct CatalogueItem<'a, 'b, 'c, 'd> {
    pub object: &'a ObjectListItem,
    pub conflict: Vec<ConflictingResource<'b, 'c>>,
    pub nodes: Vec<&'d NodeState>,
}

#[cfg(test)]
mod tests {
    use crate::filestore::ObjectListItem;
    use crate::skatelet::database::resource::ResourceType;
    use crate::skatelet::SystemInfo;
    use crate::ssh::HostInfo;
    use crate::state::state::{ClusterState, NodeState, NodeStatus};
    use crate::util::NamespacedName;

    #[test]
    fn should_detect_conflicts() {
        let state = ClusterState {
            cluster_name: "test".to_string(),
            nodes: vec![
                NodeState {
                    node_name: "node1".to_string(),
                    status: NodeStatus::Healthy,
                    message: None,
                    host_info: Some(HostInfo {
                        node_name: "node1".to_string(),
                        system_info: Some(SystemInfo {
                            resources: vec![
                                ObjectListItem {
                                    name: "same-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 1,
                                    ..Default::default()
                                },
                                ObjectListItem {
                                    name: "lesser-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 2,
                                    ..Default::default()
                                },
                                ObjectListItem {
                                    name: "greater-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 1,
                                    ..Default::default()
                                },
                            ],
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
                NodeState {
                    node_name: "node2".to_string(),
                    status: NodeStatus::Healthy,
                    message: None,
                    host_info: Some(HostInfo {
                        node_name: "node2".to_string(),
                        system_info: Some(SystemInfo {
                            resources: vec![
                                ObjectListItem {
                                    name: "same-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 1,
                                    ..Default::default()
                                },
                                ObjectListItem {
                                    name: "lesser-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 1,
                                    ..Default::default()
                                },
                                ObjectListItem {
                                    name: "greater-version.ns".into(),
                                    resource_type: ResourceType::Pod,
                                    generation: 2,
                                    ..Default::default()
                                },
                            ],
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ],
        };

        let same_version =
            state.catalogue(None, &[ResourceType::Pod], Some("ns"), Some("same-version"));
        assert_eq!(same_version.len(), 1);
        assert_eq!(
            same_version[0].object.name.to_string(),
            "same-version.ns".to_string()
        );
        assert_eq!(same_version[0].conflict.len(), 0);

        let lesser_version = state.catalogue(
            None,
            &[ResourceType::Pod],
            Some("ns"),
            Some("lesser-version"),
        );
        assert_eq!(lesser_version.len(), 1);
        assert_eq!(
            lesser_version[0].object.name.to_string(),
            "lesser-version.ns".to_string()
        );
        assert_eq!(lesser_version[0].object.generation, 2);
        assert_eq!(lesser_version[0].conflict.len(), 1);
        assert_eq!(
            lesser_version[0].conflict[0].object.name.to_string(),
            "lesser-version.ns".to_string()
        );
        assert_eq!(lesser_version[0].conflict[0].nodes.len(), 1);
        assert_eq!(
            lesser_version[0].conflict[0].nodes[0].node_name,
            "node2".to_string()
        );
        assert_eq!(lesser_version[0].conflict[0].object.generation, 1);

        let greater_version = state.catalogue(
            None,
            &[ResourceType::Pod],
            Some("ns"),
            Some("greater-version"),
        );
        assert_eq!(greater_version.len(), 1);
        assert_eq!(
            greater_version[0].object.name.to_string(),
            "greater-version.ns".to_string()
        );
        assert_eq!(greater_version[0].object.generation, 2);
        assert_eq!(greater_version[0].conflict.len(), 1);
        assert_eq!(
            greater_version[0].conflict[0].object.name.to_string(),
            "greater-version.ns".to_string()
        );
        assert_eq!(greater_version[0].conflict[0].nodes.len(), 1);
        assert_eq!(
            greater_version[0].conflict[0].nodes[0].node_name,
            "node1".to_string()
        );
        assert_eq!(greater_version[0].conflict[0].object.generation, 1);
    }
}
