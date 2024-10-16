use anyhow::anyhow;
use clap::Args;
use itertools::{Either, Itertools};
use crate::config::Config;
use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use crate::ssh;

use crate::ssh::SshClients;
use crate::state::state::{NodeStatus, ClusterState};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};

#[derive(Debug, Args)]
pub struct RefreshArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, long_help = "print state as json to stdout")]
    json: bool,
}


pub async fn refresh(args: RefreshArgs) -> Result<(), SkateError> {
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


    if args.json {
        serde_json::to_writer(std::io::stdout(), &state)?;
    } else {
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
    }


    Ok(())
}


pub async fn refreshed_state(cluster_name: &str, conns: &SshClients, config: &Config) -> Result<ClusterState, SkateError> {
    let host_infos = conns.get_nodes_system_info().await;
    let (healthy_host_infos, errors): (Vec<_>, Vec<SkateError>) = host_infos.into_iter().partition_map(|r|
        match r {
            Ok(r) => Either::Left(r),
            Err(e) => Either::Right(e.into()),
        }
    );

    if errors.len() > 0 {
        return Err(SkateError::Multi(errors));
    }


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