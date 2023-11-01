use std::error::Error;
use anyhow::anyhow;
use itertools::{Either, Itertools};
use clap::Args;
use crate::config::Node;
use crate::skate::ConfigFileArgs;
use crate::ssh;
use crate::ssh::{HostInfoResponse};

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: ConfigFileArgs,
    #[arg(long, long_help = "Url prefix where to find binaries", default_value = "https://skate.on/releases/", env)]
    binary_url_prefix: String,
}



pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let config = crate::skate::read_config(args.hosts.skateconfig)?;
    let cluster = config.current_cluster()?;


    let (clients, errors) = ssh::connections(&cluster).await;

    if errors.is_some() {
        eprintln!();
        eprintln!("{}", errors.expect("should have had errors"))
    }

    if clients.is_none() {
        return Err(anyhow!("failed to connect to any hosts").into());
    }
    let clients = clients.expect("should have had clients");

    let results = clients.get_hosts_info().await;

    for result in results {
        match result {
            Ok(info) => {
                println!("✅  {} ({:?} - {}) running skatelet version {}",
                         info.hostname,
                         info.platform.os,
                         info.platform.arch,
                         info.skatelet_version.unwrap_or_default().split_whitespace().last().unwrap_or_default())
            }
            Err(err) => {
                eprintln!("{}", err)
            }
        }
    }

    Ok(())
}
