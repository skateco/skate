use std::any::Any;
use std::env::consts::ARCH;
use sysinfo::{CpuRefreshKind, Networks, RefreshKind, System, SystemExt};
use std::error::Error;
use std::str::FromStr;
use clap::{Args, Subcommand};
use serde::Serialize;
use crate::skate::{Distribution, Os, Platform};
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


#[derive(Serialize)]
struct SystemInfo {
    platform: Platform,
    total_memory_mib: u64,
    used_memory_mib: u64,
    total_swap_mib: u64,
    used_swap_mib: u64,
    num_cpus: usize,
}

async fn info() -> Result<(), Box<dyn Error>> {
    let sys = System::new_with_specifics(RefreshKind::new()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory()
        .with_networks()
    );
    let os = Os::from_str(&(sys.name().ok_or("")?)).unwrap_or(Os::Unknown);

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
    };
    let json = serde_json::to_string(&info)?;
    println!("{}", json);

    Ok(())
}
