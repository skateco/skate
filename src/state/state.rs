use crate::config::{cache_dir, Config};
use crate::filestore::ObjectListItem;
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::core::v1::{Node as K8sNode, NodeAddress, NodeSpec, NodeStatus as K8sNodeStatus, Secret, Service};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::ops::DerefMut;
use std::path::Path;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::networking::v1::Ingress;
use strum_macros::Display;
use tabled::Tabled;

use crate::skate::SupportedResources;
use crate::skatelet::system::podman::PodmanPodInfo;
use crate::skatelet::SystemInfo;
use crate::spec::cert::ClusterIssuer;
use crate::ssh::HostInfo;
use crate::state::state::NodeStatus::{Healthy, Unhealthy, Unknown};
use crate::util::{hash_string, metadata_name, slugify};

#[derive(Serialize, Deserialize, Clone, Debug, Display, PartialEq)]
pub enum NodeStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Tabled, Serialize, Deserialize, Debug, Clone)]
#[tabled(rename_all = "UPPERCASE")]
pub struct NodeState {
    pub node_name: String,
    pub status: NodeStatus,
    #[tabled(skip)]
    pub host_info: Option<HostInfo>,
}

impl From<NodeState> for K8sNode {
    fn from(val: NodeState) -> Self {
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

        spec.unschedulable = match val.status {
            Unknown => Some(true),
            Healthy => Some(false),
            Unhealthy => Some(true),
        };

        let sys_info = val.host_info.as_ref().and_then(|h| h.system_info.clone());


        (status.capacity, status.allocatable, status.addresses, metadata.labels) = match sys_info {
            Some(si) => {
                (Some(BTreeMap::<String, Quantity>::from([
                    ("cpu".to_string(), Quantity(format!("{}", si.num_cpus))),
                    ("memory".to_string(), Quantity(format!("{} Mib", si.total_memory_mib))),
                ])),
                 (Some(BTreeMap::<String, Quantity>::from([
                     ("cpu".to_string(), Quantity(format!("{}", (si.num_cpus as f32) * (100.00 - si.cpu_usage) / 100.0))),
                     ("memory".to_string(), Quantity(format!("{} Mib", si.total_memory_mib - si.used_memory_mib))),
                 ]))), ({
                    let mut addresses = vec![
                        NodeAddress {
                            address: si.hostname.clone(),
                            type_: "Hostname".to_string(),
                        },
                    ];
                    if let Some(ip) = si.internal_ip_address {
                        addresses.push(NodeAddress {
                            address: ip,
                            type_: "InternalIP".to_string(),
                        })
                    }
                    Some(addresses)
                }), (
                     Some(BTreeMap::<String, String>::from([
                         ("skate.io/arch".to_string(), si.platform.arch.clone()),
                         ("skate.io/nodename".to_string(), val.node_name.clone()),
                         ("skate.io/hostname".to_string(), si.hostname.clone()),
                     ]))
                 ))
            }
            None => (None, None, None, None)
        };


        K8sNode {
            metadata,
            spec: Some(spec),
            status: Some(status),
        }
    }
}

impl NodeState {
    pub fn filter_pods(&self, f: &dyn Fn(&PodmanPodInfo) -> bool) -> Vec<PodmanPodInfo> {
        self.host_info.as_ref().and_then(|h| {
                h.system_info.clone().and_then(|i| {
                    i.pods.map(|p| p.clone().into_iter().filter(|p| f(p)).collect::<Vec<_>>())
                })
        }).unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClusterState {
    pub cluster_name: String,
    pub hash: String,
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
        ReconciledResult{
            removed: 1,
            added: 0,
            updated: 0,
        }
    }
    fn added() -> ReconciledResult {
        ReconciledResult{
            removed: 0,
            added: 1,
            updated: 0,
        }
    }
    fn updated() -> ReconciledResult {
        ReconciledResult{
            removed: 0,
            added: 0,
            updated: 1,
        }
    }
}

impl ClusterState {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(ClusterState::path(&self.cluster_name.clone()).as_str()))
            .map_err(|e| anyhow!("failed to open or create state file").context(e))?;
        serde_json::to_writer(state_file, self)
            .map_err(|e| anyhow!("failed to serialize state").context(e))?;
        Ok(())
    }

    pub fn load(cluster_name: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::create(ClusterState::path(cluster_name)).map_err(|e| anyhow!("failed to open or create state file").context(e))?;

        let result = serde_json::from_reader::<_, ClusterState>(file).map_err(|e| anyhow!("failed to parse cluster state").context(e));

        match result {
            Ok(state) => Ok(state),
            Err(_e) => {
                let state = ClusterState {
                    cluster_name: cluster_name.to_string(),
                    hash: "".to_string(),
                    nodes: vec![],
                };
                Ok(state)
            }
        }
    }

    pub fn reconcile_node(&mut self, node: &HostInfo) -> Result<ReconciledResult, Box<dyn Error>> {
        let pos = self.nodes.iter_mut().find_position(|n| n.node_name == node.node_name);

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

    pub fn reconcile_object_creation(&mut self, object: &SupportedResources, node_name: &str) -> Result<ReconciledResult, Box<dyn Error>> {
        let node = self.nodes.iter_mut().find(|n| n.node_name == node_name)
            .ok_or(anyhow!("node not found: {}", node_name))?;

        match object {
            SupportedResources::Pod(pod) => Self::reconcile_pod_creation(&PodmanPodInfo::from((*pod).clone()), node),
            SupportedResources::Ingress(ingress) => Self::reconcile_ingress_creation(ingress, node),
            SupportedResources::CronJob(cronjob) => Self::reconcile_cronjob_creation(cronjob, node),
            SupportedResources::Secret(secret) => Self::reconcile_secret_creation(secret, node),
            SupportedResources::Service(service) => Self::reconcile_service_creation(service, node),
            SupportedResources::ClusterIssuer(issuer) => Self::reconcile_cluster_issuer_creation(issuer, node),
            // This is a no-op since the only thing that happens when during the Deployment's ScheduledOperation is that we write the manifest to file for future reference
            // The state change is all done by the Pods' scheduled operations
            SupportedResources::Deployment(_) => {/* nothing to do */Ok(ReconciledResult::default())},
            _ => todo!("reconcile not supported")
        }
    }

    pub fn reconcile_object_deletion(&mut self, object: &SupportedResources, node_name: &str) -> Result<ReconciledResult, Box<dyn Error>> {
        let node = self.nodes.iter_mut().find(|n| n.node_name == node_name)
            .ok_or(anyhow!("node not found: {}", node_name))?;

        match object {
            SupportedResources::Pod(pod) => Self::reconcile_pod_deletion(&PodmanPodInfo::from((*pod).clone()), node),
            SupportedResources::Ingress(ingress) => Self::reconcile_ingress_deletion(ingress, node),
            SupportedResources::CronJob(cronjob) => Self::reconcile_cronjob_deletion(cronjob, node),
            SupportedResources::Secret(secret) => Self::reconcile_secret_deletion(secret, node),
            SupportedResources::Service(service) =>Self::reconcile_service_deletion(service,node),
            SupportedResources::ClusterIssuer(issuer) => Self::reconcile_cluster_issuer_deletion(issuer, node),
            SupportedResources::Deployment(deployment) => Self::reconcile_deployment_deletion(deployment, node),
            SupportedResources::DaemonSet(daemonset) => Self::reconcile_daemonset_deletion(daemonset, node),
            _ => todo!("reconcile not supported")
        }
    }


    fn reconcile_cluster_issuer_creation(issuer: &ClusterIssuer, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.cluster_issuers.as_mut().map(|i| i.push(ObjectListItem::from(issuer)))
            })
        });

        Ok(ReconciledResult::added())
    }
    fn reconcile_cluster_issuer_deletion(issuer: &ClusterIssuer, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.cluster_issuers.as_mut().map(|i| i.retain(|i| i.name != metadata_name(issuer)))
            })
        });

        Ok(ReconciledResult::removed())
    }
    fn reconcile_service_creation(service: &Service, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.services.as_mut().map(|i| i.push(ObjectListItem::from(service)))
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_service_deletion(service: &Service, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.services.as_mut().map(|i| i.retain(|i| i.name != metadata_name(service)))
            })
        });

        Ok(ReconciledResult::removed())
    }

    fn reconcile_secret_creation(secret: &Secret, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.secrets.as_mut().map(|i| i.push(ObjectListItem::from(secret)))
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_secret_deletion(secret: &Secret, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.secrets.as_mut().map(|i| i.retain(|i| i.name != metadata_name(secret)))
            })
        });

        Ok(ReconciledResult::removed())
    }

    fn reconcile_cronjob_creation(cronjob: &CronJob, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.cronjobs.as_mut().map(|i| i.push(ObjectListItem::from(cronjob)))
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_cronjob_deletion(cronjob: &CronJob, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.cronjobs.as_mut().map(|i| i.retain(|c| c.name != metadata_name(cronjob)))
            })
        });

        Ok(ReconciledResult::removed())
    }


    fn reconcile_ingress_creation(ingress: &Ingress, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.ingresses.as_mut().map(|i| i.push(ObjectListItem::from(ingress)))
            })
        });
        Ok(ReconciledResult::added())
    }

    fn reconcile_ingress_deletion(ingress: &Ingress, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.ingresses.as_mut().map(|items|
                    items.retain(|existing| existing.name != metadata_name(ingress))
                )
            })
        });
        Ok(ReconciledResult::removed())
    }


    fn reconcile_pod_creation(pod: &PodmanPodInfo, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().map(|pods| {
                    pods.push(pod.clone());
                })
            })
        });

        Ok(ReconciledResult::added())
    }

    fn reconcile_pod_deletion(pod: &PodmanPodInfo, mut node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        node.deref_mut().host_info.as_mut().and_then(|hi| {
            hi.system_info.as_mut().and_then(|si| {
                si.pods.as_mut().map(|pods| {
                    pods.retain(|p| p.name != pod.name)
                })
            })
        });

        Ok(ReconciledResult::removed())
    }

    fn reconcile_daemonset_deletion(daemonset: &DaemonSet, node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        let name = metadata_name(daemonset).to_string();
        node.host_info.as_mut().and_then(|hi| hi.system_info.as_mut().and_then(|si|
            si.pods.as_mut().map(|pods| pods.iter().filter(|p|!p.daemonset().is_empty() &&  p.daemonset() != name))
        ));
        Ok(ReconciledResult::removed())
    }

    fn reconcile_deployment_deletion(deployment: &Deployment, node: &mut NodeState) -> Result<ReconciledResult, Box<dyn Error>> {
        let name = metadata_name(deployment).to_string();
        node.host_info.as_mut().and_then(|hi| hi.system_info.as_mut().and_then(|si|
            si.pods.as_mut().map(|pods| pods.iter().filter(|p| !p.deployment().is_empty() && p.deployment() != name))
        ));
        Ok(ReconciledResult::removed())
    }

    pub fn reconcile_all_nodes(&mut self, cluster_name: &str, config: &Config, host_info: &[HostInfo]) -> Result<ReconciledResult, Box<dyn Error>> {
        let cluster = config.active_cluster(Some(cluster_name.to_string()))?;
        self.hash = hash_string(cluster);

        let state_hosts: HashSet<String> = self.nodes.iter().map(|n| n.node_name.clone()).collect();

        let config_hosts: HashSet<String> = cluster.nodes.iter().map(|n| n.name.clone()).collect();


        let new = &config_hosts - &state_hosts;
        let orphaned = &state_hosts - &config_hosts;


        self.nodes = self.nodes.iter().filter_map(|n| {
            match orphaned.contains(&n.node_name) {
                false => Some(n.clone()),
                true => None
            }
        }).collect();

        let mut new_nodes: Vec<NodeState> = config.active_cluster(Some(cluster_name.to_string()))?.nodes.iter().filter_map(|n| {
            match new.contains(&n.name) {
                true => Some(NodeState {
                    node_name: n.name.clone(),
                    status: Unknown,
                    host_info: None,
                }),
                false => None
            }
        }).collect();

        self.nodes.append(&mut new_nodes);


        let mut updated = 0;
        // now that we have our list, go through and mark them healthy or unhealthy
        self.nodes = self.nodes.iter().map(|node| {
            let mut node = node.clone();
            match host_info.iter().find(|h| h.node_name == node.node_name) {
                Some(info) => {
                    updated += 1;
                    node.status = match info.healthy() {
                        true => Healthy,
                        false => Unhealthy
                    };
                    node.host_info = Some(info.clone())
                }
                None => {
                    node.status = Unknown;
                }
            };
            node
        }).collect();


        Ok(ReconciledResult {
            removed: orphaned.len(),
            added: new.len(),
            updated,
        })
    }

    pub fn filter_pods(&self, f: &dyn Fn(&PodmanPodInfo) -> bool) -> Vec<(PodmanPodInfo, &NodeState)> {
        let res: Vec<_> = self.nodes.iter().filter_map(|n| {
            Some(n.filter_pods(&|p| f(p)).into_iter().map(|p| (p, n)).collect::<Vec<_>>())
        }).flatten().collect();
        res
    }

    pub fn locate_daemonset(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        self.filter_pods(&|p| p.name == name && p.namespace() == namespace && p.labels.contains_key("skate.io/daemonset"))
    }

    pub fn locate_pods(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        // the pod manifest name is what podman will use to name the pod, thus has the namespace in it as a suffix
        self.filter_pods(&|p| p.name == format!("{}.{}", name, namespace) && p.namespace() == namespace)
    }


    pub fn locate_objects(&self, node: Option<&str>, selector: impl Fn(&SystemInfo) -> Option<Vec<ObjectListItem>>, name: &str, namespace: &str) -> Vec<(ObjectListItem, &NodeState)> {
        self.nodes.iter().filter(|n| node.is_none() || n.node_name == node.unwrap()).filter_map(|n| {
            n.host_info.as_ref().and_then(|h| {
                h.system_info.clone().and_then(|i| {
                    selector(&i).and_then(|p| {
                        p.clone().into_iter().find(|p| {
                            p.name.name == name && p.name.namespace == namespace
                        }).map(|o| (o, n))
                    })
                })
            })
        }).collect()
    }


    pub fn locate_deployment(&self, name: &str, namespace: &str) -> Vec<(PodmanPodInfo, &NodeState)> {
        let name = name.strip_prefix(format!("{}.", namespace).as_str()).unwrap_or(name);
        self.filter_pods(&|p| p.deployment() == name && p.namespace() == namespace)
    }

}
