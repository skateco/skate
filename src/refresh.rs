use std::error::Error;
use anyhow::anyhow;
use clap::Args;
use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh;

use crate::ssh::SshClients;
use crate::state::state::{NodeStatus, ClusterState};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};

#[derive(Debug, Args)]
pub struct RefreshArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, long_help = "Url prefix where to find binaries", default_value = "https://skate.on/releases/", env)]
    binary_url_prefix: String,
}


pub async fn refresh(args: RefreshArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let cluster = config.active_cluster(args.config.context)?;


    let (clients, errors) = ssh::cluster_connections(cluster).await;

    if errors.is_some() {
        eprintln!();
        eprintln!("{}", errors.expect("should have had errors"))
    }

    if clients.is_none() {
        return Err(anyhow!("failed to connect to any hosts").into());
    }
    let clients = clients.expect("should have had clients");

    let state = refreshed_state(&cluster.name, &clients, &config).await.expect("failed to refresh state");

    for node in &(state.nodes) {
        let emoji = match node.status {
            NodeStatus::Unhealthy => {
                CROSS_EMOJI
            }
            NodeStatus::Healthy => {
                CHECKBOX_EMOJI
            }
            NodeStatus::Unknown => {
                ' '
            }
        };
        println!("node {} {} - {} ", node.node_name, node.status, emoji)
    }

    Ok(())
}


pub async fn refreshed_state(cluster_name: &str, conns: &SshClients, config: &Config) -> Result<ClusterState, Box<dyn Error>> {
    let host_infos = conns.get_nodes_system_info().await;
    let healthy_host_infos: Vec<_> = host_infos.iter().filter_map(|h| match h {
        Ok(r) => Some((*r).clone()),
        Err(e) => {
            eprintln!("error getting host info: {}", e);
            None
        },
    }).collect();


    let mut state = match ClusterState::load(cluster_name) {
        Ok(state) => state,
        Err(_) => ClusterState {
            cluster_name: cluster_name.to_string(),
            hash: "".to_string(),
            nodes: vec![],
        }
    };

    let _ = state.reconcile_all_nodes(cluster_name, config, &healthy_host_infos)?;
    Ok(state)
}