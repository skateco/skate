use crate::config::{Config, Node};
use crate::skate::{ConfigFileArgs, SupportedResources};
use crate::ssh::{node_connection, SshClients};
use anyhow::anyhow;
use clap::{Args, Subcommand};
use std::error::Error;
use crate::create::CreateArgs;
use crate::errors::SkateError;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::state::state::ClusterState;

#[derive(Debug, Args)]
pub struct ClusterArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(long_about="Re-apply all resources in the cluster. Useful after cordon/uncordon or node creation")]
    Reschedule(RescheduleArgs),
}

pub async fn cluster(args: ClusterArgs) -> Result<(), SkateError> {
    Ok(())
}

#[derive(Debug, Args)]
pub struct RescheduleArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn reschedule(args: RescheduleArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    // get all resources from the cluster
    // - secrets
    // - deployments
    // - daemonsets
    // - services
    // - ingress
    
    // for each one, do an `apply`

    Ok(())
}

async fn propagate_existing_resources(all_conns: &SshClients, exclude_donor_node: Option<&str>, state: &mut ClusterState) -> Result<(), Box<dyn Error>> {
    // TODO - search all nodes, not just one random
    let donor_state = match state.nodes.iter().find(|n| 
        (exclude_donor_node.is_none() ||  n.node_name != exclude_donor_node.unwrap()) 
            && n.host_info.as_ref().and_then(|h| h.system_info.as_ref()).is_some()) {
        Some(n) => n,
        None => return Ok(())
    };


    let donor_sys_info = donor_state.host_info.as_ref().and_then(|h| h.system_info.as_ref()).unwrap();

    let services: Vec<_> = donor_sys_info.services.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let secrets: Vec<_> = donor_sys_info.secrets.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let deployments: Vec<_> = donor_sys_info.deployments.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let daemonsets: Vec<_> = donor_sys_info.daemonsets.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    // TODO - do we want to do cronjobs too?
    let ingresses: Vec<_> = donor_sys_info.ingresses.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();

    let all_manifests: Vec<_> = [services, secrets, deployments, daemonsets, ingresses].concat().iter().filter_map(|i| SupportedResources::try_from(i.clone()).ok()).collect();
    println!("propagating {} resources", all_manifests.len());


    let mut filtered_state = state.clone();
    filtered_state.nodes = vec!(state.nodes.iter().find(|n|
        exclude_donor_node.is_none() ||  n.node_name != exclude_donor_node.unwrap()
    ).cloned().unwrap());


    let scheduler = DefaultScheduler {};

    scheduler.schedule(all_conns, &mut filtered_state, all_manifests).await?;

    Ok(())
}
