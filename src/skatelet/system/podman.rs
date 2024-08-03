use std::collections::{BTreeMap, HashMap};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use k8s_openapi::api::core::v1::{Pod, PodSpec, PodStatus as K8sPodStatus, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use strum_macros::{Display, EnumString};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecret {
    #[serde(rename = "ID")]
    pub id: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub spec: PodmanSecretSpec,
    pub secret_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecretSpec {
    pub name: String,
    pub driver: PodmanSecretDriver,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecretDriver {
    pub name: String,
    pub options: HashMap<String, String>,
}

#[derive(Clone, Debug, EnumString, Display, Serialize, Deserialize, PartialEq)]
pub enum PodmanPodStatus {
    Created,
    Running,
    Stopped,
    Exited,
    Dead,
    Degraded,
    Error,
}

impl PodmanPodStatus {
    fn to_pod_phase(self) -> String {
        match self {
            PodmanPodStatus::Running => "Running",
            PodmanPodStatus::Stopped => "Succeeded",
            PodmanPodStatus::Exited => "Succeeded",
            PodmanPodStatus::Dead => "Failed",
            PodmanPodStatus::Degraded => "Running",
            PodmanPodStatus::Created => "Pending",
            PodmanPodStatus::Error => "Failed",
        }.to_string()
    }
    fn from_pod_phase(phase: &str) -> Self {
        match phase {
            "Running" => PodmanPodStatus::Running,
            // lossy
            "Succeeded" => PodmanPodStatus::Exited,
            "Failed" => PodmanPodStatus::Dead,
            "Pending" => PodmanPodStatus::Created,
            _ => PodmanPodStatus::Created,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PodmanPodInfo {
    pub id: String,
    pub name: String,
    pub status: PodmanPodStatus,
    pub created: DateTime<Local>,
    pub labels: BTreeMap<String, String>,
    pub containers: Option<Vec<PodmanContainerInfo>>,
}


impl PodmanPodInfo {
    pub fn namespace(&self) -> String {
        self.labels.get("skate.io/namespace").map(|ns| ns.clone()).unwrap_or("".to_string())
    }
    pub fn deployment(&self) -> String {
        self.labels.get("skate.io/deployment").map(|d| d.clone()).unwrap_or("".to_string())
    }
}

impl From<Pod> for PodmanPodInfo {
    fn from(value: Pod) -> Self {
        PodmanPodInfo {
            id: value.metadata.uid.unwrap_or("".to_string()),
            name: value.metadata.name.unwrap_or("".to_string()),
            status: PodmanPodStatus::from_pod_phase(value.status.and_then(|s| s.phase.and_then(|p| {
                Some(p)
            })).unwrap_or("".to_string()).as_str()),
            created: value.metadata.creation_timestamp.and_then(|ts| Some(DateTime::from(ts.0))).unwrap_or(DateTime::from(Local::now())),
            labels: value.metadata.labels.unwrap_or(BTreeMap::new()),
            containers: None, // TODO
        }
    }
}

impl Into<Pod> for PodmanPodInfo {
    fn into(self) -> Pod {
        Pod {
            metadata: ObjectMeta {
                annotations: None,
                creation_timestamp: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(DateTime::from(self.created))),
                deletion_grace_period_seconds: None,
                deletion_timestamp: None,
                finalizers: None,
                generate_name: None,
                generation: None,
                labels: match self.labels.len() {
                    0 => None,
                    _ => Some(self.labels.iter().filter_map(|(k, v)| {
                        if k.starts_with("nodeselector/") {
                            None
                        } else {
                            Some((k.clone(), v.clone()))
                        }
                    }).collect())
                },
                managed_fields: None,
                name: Some(self.name.clone()),
                namespace: Some(self.namespace()),
                owner_references: None,
                resource_version: None,
                self_link: None,
                uid: Some(self.id),
            },
            spec: Some(PodSpec {
                active_deadline_seconds: None,
                affinity: None,
                automount_service_account_token: None,
                containers: vec![],
                dns_config: None,
                dns_policy: None,
                enable_service_links: None,
                ephemeral_containers: None,
                host_aliases: None,
                host_ipc: None,
                host_network: None,
                host_pid: None,
                host_users: None,
                hostname: None,
                image_pull_secrets: None,
                init_containers: None,
                node_name: None,
                node_selector: Some(self.labels.iter().filter_map(|(k, v)| {
                    if k.starts_with("nodeselector/") {
                        Some(((*k.clone()).strip_prefix("nodeselector/").unwrap_or(k).to_string(), (*v).clone()))
                    } else {
                        None
                    }
                }).collect::<BTreeMap<String, String>>()),
                os: None,
                overhead: None,
                preemption_policy: None,
                priority: None,
                priority_class_name: None,
                readiness_gates: None,
                resource_claims: None,
                restart_policy: None,
                runtime_class_name: None,
                scheduler_name: None,
                scheduling_gates: None,
                security_context: None,
                service_account: None,
                service_account_name: None,
                set_hostname_as_fqdn: None,
                share_process_namespace: None,
                subdomain: None,
                termination_grace_period_seconds: None,
                tolerations: None,
                topology_spread_constraints: None,
                volumes: None,
            }), // TODO
            status: Some(K8sPodStatus
            {
                conditions: None,
                container_statuses: None,
                ephemeral_container_statuses: None,
                host_ip: None,
                host_ips: None,
                init_container_statuses: None,
                message: None,
                nominated_node_name: None,
                phase: Some(self.status.to_pod_phase()),
                pod_ip: None,
                pod_ips: None,
                qos_class: None,
                reason: None,
                resize: None,
                resource_claim_statuses: None,
                start_time: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PodmanContainerInfo {
    pub id: String,
    pub names: String,
    pub status: String,
    pub restart_count: Option<usize>,
}
