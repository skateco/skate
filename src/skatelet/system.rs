use std::collections::{BTreeMap};
use std::env::consts::ARCH;
use sysinfo::{CpuRefreshKind, DiskKind, Disks, MemoryRefreshKind, RefreshKind, System};
use std::error::Error;


use anyhow::anyhow;
use chrono::{DateTime, Local};
use clap::{Args, Subcommand};
use k8s_openapi::api::core::v1::{Pod, PodSpec, PodStatus as K8sPodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};
use crate::filestore::{FileStore, ObjectListItem};

use crate::skate::{Distribution, exec_cmd, Os, Platform};



#[derive(Debug, Args)]
pub struct SystemArgs {
    #[command(subcommand)]
    command: SystemCommands,
}


#[derive(Debug, Subcommand)]
pub enum SystemCommands {
    #[command(about = "report system information")]
    Info,
}

pub async fn system(args: SystemArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        SystemCommands::Info => info().await?
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub available_space_mib: u64,
    pub total_space_mib: u64,
    pub disk_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub platform: Platform,
    pub total_memory_mib: u64,
    pub used_memory_mib: u64,
    pub total_swap_mib: u64,
    pub used_swap_mib: u64,
    pub num_cpus: usize,
    pub root_disk: Option<DiskInfo>,
    pub pods: Option<Vec<PodmanPodInfo>>,
    pub ingresses: Option<Vec<ObjectListItem>>,
    pub cronjobs: Option<Vec<ObjectListItem>>,
    pub cpu_freq_mhz: u64,
    pub cpu_usage: f32,
    pub cpu_brand: String,
    pub cpu_vendor_id: String,
    pub internal_ip_address: Option<String>,
    pub external_ip_address: Option<String>,
    pub hostname: String,
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

// TODO - have more generic ObjectMeta type for explaining existing resources

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

// returns (external, internal)
fn get_ips(os: &Os) -> Result<(Option<String>, Option<String>), Box<dyn Error>> {
    let iface_cmd = match os {
        Os::Unknown => None,
        Os::Darwin | Os::Linux => Some("ifconfig -a | awk '
/^[a-zA-Z0-9_\\-]+:/ {
  sub(/:/, \"\");iface=$1}
/^[[:space:]]*inet / {
  split($2, a, \"/\")
  print iface\"  \"a[1]
}'"),
    };

    let iface_ips: Vec<_> = match iface_cmd {
        Some(cmd) => {
            exec_cmd("bash", &["-c", cmd])
                .map(|s| s.split("\n")
                    .map(|l| l.split("  ").collect::<Vec<&str>>())
                    .filter(|l| l.len() == 2)
                    .map(|l| (l[0].to_string(), l[1].to_string())).collect())
                .map_err(|e| anyhow!("failed to get ips: {}", e))?
        }
        None => {
            vec!()
        }
    };

    let external_ip = iface_ips.iter().find(|(iface, _)| {
        match os {
            Os::Darwin => iface == "en0",
            Os::Linux => iface == "eth0",
            _ => false
        }
    }).map(|(_, ip)| ip.clone()).unwrap_or("".to_string());

    Ok((Some(external_ip), None))
}

const BYTES_IN_MIB: u64 = (2u64).pow(20);

async fn info() -> Result<(), Box<dyn Error>> {
    let sys = System::new_with_specifics(RefreshKind::new()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory(MemoryRefreshKind::everything())
    );
    let os = Os::from_str_loose(&(System::name().ok_or("")?));

    let result = match exec_cmd(
        "sudo",
        &["podman", "pod", "ps", "--filter", "label=skate.io/namespace", "--format", "json"],
    ) {
        Ok(result) => match result.as_str() {
            "" => "[]".to_string(),
            "null" => "[]".to_string(),
            _ => result
        },
        Err(err) => {
            eprintln!("failed to list pods: {}", err);
            "[]".to_string()
        }
    };

    let podman_pod_info: Vec<PodmanPodInfo> = serde_json::from_str(&result).map_err(|e| anyhow!(e).context("failed to deserialize pod info"))?;


    let store = FileStore::new();
    // list ingresses
    let ingresses = store.list_objects("ingress")?;
    let cronjobs = store.list_objects("cronjob")?;


    let iface_ipv4 = match get_ips(&os) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("failed to get interface ipv4 addresses: {}", e);
            (None, None)
        }
    };


    let root_disk = Disks::new_with_refreshed_list().iter().find(|d| d.mount_point().to_string_lossy() == "/")
        .and_then(|d| Some(DiskInfo {
            available_space_mib: d.available_space() / BYTES_IN_MIB,
            total_space_mib: d.total_space() / BYTES_IN_MIB,
            disk_kind: match d.kind() {
                DiskKind::HDD => "hdd",
                DiskKind::SSD => "sdd",
                DiskKind::Unknown(_) => "unknown"
            }.to_string(),
        }));


    let info = SystemInfo {
        platform: Platform {
            arch: ARCH.to_string(),
            os,
            distribution: Distribution::Unknown, // TODO
        },
        total_memory_mib: sys.total_memory() / BYTES_IN_MIB,
        used_memory_mib: sys.used_memory() / BYTES_IN_MIB,
        total_swap_mib: sys.total_swap() / BYTES_IN_MIB,
        used_swap_mib: sys.used_swap() / BYTES_IN_MIB,
        num_cpus: sys.cpus().len(),
        cpu_freq_mhz: sys.global_cpu_info().frequency(),
        cpu_usage: sys.global_cpu_info().cpu_usage(),
        cpu_brand: sys.global_cpu_info().brand().to_string(),
        cpu_vendor_id: sys.global_cpu_info().vendor_id().to_string(),
        root_disk,
        pods: Some(podman_pod_info),
        ingresses: match ingresses.is_empty() {
            true => None,
            false => Some(ingresses),
        },
        cronjobs: match cronjobs.is_empty() {
            true => None,
            false => Some(cronjobs),
        },
        hostname: sysinfo::System::host_name().unwrap_or("".to_string()),
        external_ip_address: iface_ipv4.0,
        internal_ip_address: iface_ipv4.1,
    };
    let json = serde_json::to_string(&info)?;
    println!("{}", json);

    Ok(())
}
