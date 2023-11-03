use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use anyhow::anyhow;
use itertools::{Either, Itertools};
use clap::Args;
use crate::config::{cache_dir, Config, Node};
use crate::skate::{ConfigFileArgs, NodeState, NodeStatus, State};
use crate::ssh;
use crate::ssh::{HostInfoResponse};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::Path;
use crate::util::slugify;

#[derive(Debug, Args)]
pub struct RefreshArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, long_help = "Url prefix where to find binaries", default_value = "https://skate.on/releases/", env)]
    binary_url_prefix: String,
}


pub async fn refresh(args: RefreshArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let cluster = config.current_cluster()?;


    let (clients, errors) = ssh::cluster_connections(&cluster).await;

    if errors.is_some() {
        eprintln!();
        eprintln!("{}", errors.expect("should have had errors"))
    }

    if clients.is_none() {
        return Err(anyhow!("failed to connect to any hosts").into());
    }
    let clients = clients.expect("should have had clients");

    let results = clients.get_hosts_info().await;

    let mut state = State {
        cluster_name: cluster.name.clone(),
        hash: cluster.hash_string(),
        nodes: vec![],
    };

    for result in results {
        state.nodes.push(match result {
            Ok(info) => {
                println!("✅  {} ({:?} - {}) running skatelet version {}",
                         info.hostname,
                         info.platform.os,
                         info.platform.arch,
                         info.skatelet_version.unwrap_or_default().split_whitespace().last().unwrap_or_default());
                NodeState {
                    node_name: info.node_name,
                    status: NodeStatus::Healthy,
                    inventory_found: true,
                    inventory: vec![],
                }
            }
            Err(err) => {
                eprintln!("{}", err);
                NodeState {
                    node_name: "".parse().unwrap(),
                    status: NodeStatus::Unhealthy,
                    inventory_found: false,
                    inventory: vec![],
                }
            }
        })
    }

    let _ = state.persist().expect("failed to save state");

    Ok(())
}
