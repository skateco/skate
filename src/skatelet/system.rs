use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::env::consts::ARCH;
use sysinfo::{CpuRefreshKind, Networks, RefreshKind, System, SystemExt};
use std::error::Error;
use std::str::FromStr;
use chrono::{DateTime, Local};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use crate::skate::{Distribution, exec_cmd, Os, Platform};
use crate::util::TARGET;

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
pub struct SystemInfo {
    pub platform: Platform,
    pub total_memory_mib: u64,
    pub used_memory_mib: u64,
    pub total_swap_mib: u64,
    pub used_swap_mib: u64,
    pub num_cpus: usize,
    pub pods: Option<Vec<PodmanPodInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PodmanPodInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created: DateTime<Local>,
    pub labels: HashMap<String, String>,
    pub containers: Vec<PodmanContainerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PodmanContainerInfo {
    pub id: String,
    pub names: String,
    pub status: String,
    pub restart_count: usize,
}

async fn info() -> Result<(), Box<dyn Error>> {
    let sys = System::new_with_specifics(RefreshKind::new()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory()
        .with_networks()
    );
    let os = Os::from_str(&(sys.name().ok_or("")?)).unwrap_or(Os::Unknown);

    let result = exec_cmd(
        "podman",
        &["pod", "ps", "--filter", "label=skate.io/namespace", "--format", "json"],
    )?;
    let podman_pod_info: Vec<PodmanPodInfo> = serde_json::from_str(&result)?;


    let info = SystemInfo {
        platform: Platform {
            arch: ARCH.to_string(),
            os,
            distribution: Distribution::Unknown, // TODO
        },
        total_memory_mib: sys.total_memory(),
        used_memory_mib: sys.used_memory(),
        total_swap_mib: sys.total_swap(),
        used_swap_mib: sys.used_swap(),
        num_cpus: sys.cpus().len(),
        pods: Some(podman_pod_info),
    };
    let json = serde_json::to_string(&info)?;
    println!("{}", json);

    Ok(())
}
