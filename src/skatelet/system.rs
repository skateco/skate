pub(crate) mod podman;

use std::env::consts::ARCH;
use std::error::Error;
use sysinfo::{DiskKind, Disks, RefreshKind, System};

use anyhow::anyhow;
use clap::{Args, Subcommand};

use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::ObjectListItem;
use crate::skate::{Distribution, Platform};
use crate::skatelet::cordon::is_cordoned;
use crate::skatelet::database::resource::{list_resources, ResourceType};
use crate::skatelet::system::podman::PodmanSecret;
use crate::util::{get_skate_label_value, NamespacedName, SkateLabels, TryVecInto};
use k8s_openapi::api::core::v1::Secret;
use log::error;
use podman::PodmanPodInfo;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

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

pub trait SystemDeps: With<dyn ShellExec> + WithDB {}

pub async fn system<D: SystemDeps>(deps: D, args: SystemArgs) -> Result<(), SkateError> {
    match args.command {
        SystemCommands::Info => info(deps.get_db(), deps.get()).await?,
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskInfo {
    pub available_space_mib: u64,
    pub total_space_mib: u64,
    pub disk_kind: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub platform: Platform,
    pub total_memory_mib: u64,
    pub used_memory_mib: u64,
    pub total_swap_mib: u64,
    pub used_swap_mib: u64,
    pub num_cpus: usize,
    pub root_disk: Option<DiskInfo>,
    pub pods: Option<Vec<PodmanPodInfo>>,
    pub resources: Vec<ObjectListItem>,
    pub cpu_freq_mhz: u64,
    pub cpu_usage: f32,
    pub cpu_brand: String,
    pub cpu_vendor_id: String,
    pub internal_ip_address: Option<String>,
    pub hostname: String,
    #[serde(default)]
    pub cordoned: bool,
}

// TODO - have more generic ObjectMeta type for explaining existing resources

// returns (external, internal)
fn internal_ip(execer: Box<dyn ShellExec>) -> Result<Option<String>, Box<dyn Error>> {
    let iface_cmd = match execer.exec("which", &["ifconfig"], None) {
        Ok(_) => Some(
            r#"ifconfig -a | awk '
/^[a-zA-Z0-9_\-]+:/ {
  sub(/:/, "");iface=$1}
/^[[:space:]]*inet / {
  split($2, a, "/")
  print iface"  "a[1]
}'"#,
        ),
        _ => Some(
            r#"ip address|awk '
/^[0-9]+: [a-zA-Z]+[a-zA-Z0-9_\-]+:/ {
  sub(/[0-9]+:/, "");sub(/:/, "");iface=$1}
/^[[:space:]]*inet / {
  split($2, a, "/")
  print iface"  "a[1]
}'"#,
        ),
    };

    let iface_ips: Vec<_> = match iface_cmd {
        Some(cmd) => execer
            .exec("bash", &["-c", cmd], None)
            .map(|s| {
                s.split("\n")
                    .map(|l| l.split("  ").collect::<Vec<&str>>())
                    .filter(|l| l.len() == 2)
                    .map(|l| (l[0].to_string(), l[1].to_string()))
                    .collect()
            })
            .map_err(|e| anyhow!("failed to get ips: {}", e))?,
        None => {
            vec![]
        }
    };

    let internal_ip = iface_ips
        .iter()
        .find(|(iface, _)| ["eth0", "eno1"].contains(&iface.as_str()))
        .map(|(_, ip)| ip.clone())
        .unwrap_or("".to_string());

    Ok(Some(internal_ip))
}

const BYTES_IN_MIB: u64 = 2u64.pow(20);

async fn info(db: SqlitePool, execer: Box<dyn ShellExec>) -> Result<(), Box<dyn Error>> {
    let sys = System::new_with_specifics(RefreshKind::everything());

    let pod_list_result = match execer.exec(
        "sudo",
        &[
            "podman",
            "pod",
            "ps",
            "--filter",
            "label=skate.io/namespace",
            "--format",
            "json",
        ],
        None,
    ) {
        Ok(result) => match result.as_str() {
            "" => "[]".to_string(),
            "null" => "[]".to_string(),
            _ => result,
        },
        Err(err) => {
            eprintln!("failed to list pods: {}", err);
            "[]".to_string()
        }
    };

    let podman_pod_info: Vec<PodmanPodInfo> = serde_json::from_str(&pod_list_result)
        .map_err(|e| anyhow!(e).context("failed to deserialize pod info"))?;

    let resources = list_resources(&db).await?.try_vec_into()?;

    let secrets = execer
        .exec("podman", &["secret", "ls", "--noheading"], None)
        .unwrap_or_else(|e| {
            eprintln!("failed to list secrets: {}", e);
            "".to_string()
        });

    let secret_names: Vec<&str> = secrets
        .split("\n")
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                return None;
            }
            let secret_name = parts[1];
            secret_name.rsplit_once(".").map(|(_, _)| secret_name)
        })
        .collect();

    let secret_json = if secret_names.is_empty() {
        "[]".to_string()
    } else {
        execer
            .exec(
                "podman",
                &[
                    vec!["secret", "inspect", "--showsecret"],
                    secret_names.clone(),
                ]
                .concat(),
                None,
            )
            .unwrap_or_else(|e| {
                error!("failed to get secret info for {:?}: {}", secret_names, e);
                "[]".to_string()
            })
    };

    let secret_info: Vec<PodmanSecret> = serde_json::from_str(&secret_json)
        .map_err(|e| anyhow!(e).context("failed to deserialize secret info"))?;

    let secret_info: Vec<ObjectListItem> = secret_info
        .iter()
        .filter_map(|s| {
            let manifest = match serde_yaml::from_str::<Secret>(&s.secret_data) {
                Ok(secret) => secret,
                Err(_) => return None,
            };

            let hash = get_skate_label_value(&manifest.metadata.labels, &SkateLabels::Hash)
                .unwrap_or("".to_string());

            // if we want to redact the secret values.
            // removing for now since we don't store the state anyway.

            // let mut k8s_secret = manifest_result.unwrap();
            // k8s_secret.data = k8s_secret.data.clone().and_then(|data| {
            //     Some(data.into_iter().map(|(k, _)| (k, ByteString{ 0: vec![] })).collect())
            // });
            //
            // k8s_secret.string_data = k8s_secret.string_data.clone().and_then(|data| {
            //     Some(data.into_iter().map(|(k, _)| (k, "".to_string())).collect())
            // });

            let yaml = serde_yaml::to_value(&manifest).unwrap();

            Some(ObjectListItem {
                resource_type: ResourceType::Secret,
                name: NamespacedName::from(s.spec.name.as_str()),
                manifest_hash: hash,
                manifest: Some(yaml),
                generation: manifest.metadata.generation.unwrap_or_default(),
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
        })
        .collect();

    let resources: Vec<_> = resources
        .into_iter()
        .filter(|item: &ObjectListItem| item.resource_type != ResourceType::Secret)
        .chain(secret_info.into_iter())
        .collect();

    let internal_ip_addr = internal_ip(execer).unwrap_or_else(|e| {
        eprintln!("failed to get interface ipv4 addresses: {}", e);
        None
    });

    let root_disk = Disks::new_with_refreshed_list()
        .iter()
        .find(|d| d.mount_point().to_string_lossy() == "/")
        .map(|d| DiskInfo {
            available_space_mib: d.available_space() / BYTES_IN_MIB,
            total_space_mib: d.total_space() / BYTES_IN_MIB,
            disk_kind: match d.kind() {
                DiskKind::HDD => "hdd",
                DiskKind::SSD => "sdd",
                DiskKind::Unknown(_) => "unknown",
            }
            .to_string(),
        });

    let info = SystemInfo {
        platform: Platform {
            arch: ARCH.to_string(),
            distribution: Distribution::Unknown, // TODO
        },
        total_memory_mib: sys.total_memory() / BYTES_IN_MIB,
        used_memory_mib: sys.used_memory() / BYTES_IN_MIB,
        total_swap_mib: sys.total_swap() / BYTES_IN_MIB,
        used_swap_mib: sys.used_swap() / BYTES_IN_MIB,
        num_cpus: sys.cpus().len(),
        cpu_freq_mhz: sys.cpus().iter().map(|c| c.frequency()).sum(),
        cpu_usage: sys.global_cpu_usage(),
        cpu_brand: sys.cpus().first().unwrap().brand().to_string(),
        cpu_vendor_id: sys.cpus().first().unwrap().vendor_id().to_string(),
        root_disk,
        pods: Some(podman_pod_info),
        resources,
        hostname: System::host_name().unwrap_or("".to_string()),
        internal_ip_address: internal_ip_addr,
        cordoned: is_cordoned(),
    };
    let json = serde_json::to_string(&info)?;
    println!("{}", json);

    Ok(())
}

impl SystemInfo {
    pub fn cpu_total_millis(&self) -> usize {
        self.num_cpus * 1000
    }

    pub fn cpu_usage_millis(&self) -> usize {
        self.cpu_total_millis() * (self.cpu_usage / 100.0) as usize
    }
}
