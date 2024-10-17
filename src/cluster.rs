use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh::{cluster_connections, SshClients};
use clap::{Args, Subcommand};
use std::error::Error;
use crate::errors::SkateError;
use crate::refresh::refreshed_state;
use crate::resource::SupportedResources;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::state::state::ClusterState;

#[derive(Debug, Args)]
pub struct ClusterArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(
        long_about = "Re-apply all resources in the cluster. Useful after cordon/uncordon or node creation"
    )]
    Reschedule(RescheduleArgs),
}

pub async fn cluster(global_args: ClusterArgs) -> Result<(), SkateError> {
    match global_args.command {
        Commands::Reschedule(args) => {
            let mut args = args;
            args.config = global_args.config;
            reschedule(args).await
        }
    }
}

#[derive(Debug, Args)]
pub struct RescheduleArgs {
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[arg(long, long_help = "Will not affect the cluster if set to true")]
    pub dry_run: bool,
}

pub async fn reschedule(args: RescheduleArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let (conns, _) = cluster_connections(&cluster).await;

    let conns = conns.ok_or("failed to get cluster connections".to_string())?;

    let mut state = refreshed_state(&cluster.name, &conns, &config).await?;

    propagate_existing_resources(&conns, None, &mut state, args.dry_run).await?;

    Ok(())
}

async fn propagate_existing_resources(all_conns: &SshClients, exclude_donor_node: Option<&str>, state: &mut ClusterState, dry_run: bool) -> Result<(), Box<dyn Error>> {

    // get all resources from the cluster
    // - secrets
    // - deployments
    // - daemonsets
    // - services
    // - ingress

    // for each one, do an `apply`

    // TODO - search all nodes, not just one random
    let donor_state = match state.nodes.iter().find(|n|
        (exclude_donor_node.is_none() || n.node_name != exclude_donor_node.unwrap())
            && n.host_info.as_ref().and_then(|h| h.system_info.as_ref()).is_some()) {
        Some(n) => n,
        None => return Ok(())
    };
    


    let donor_sys_info = donor_state.host_info.as_ref().and_then(|h| h.system_info.as_ref()).unwrap();

    let services: Vec<_> = donor_sys_info.services.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let secrets: Vec<_> = donor_sys_info.secrets.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let deployments: Vec<_> = donor_sys_info.deployments.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let daemonsets: Vec<_> = donor_sys_info.daemonsets.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();
    let cronjobs: Vec<_> = state.locate_objects(None, |s| s.cronjobs.clone(), None,None).into_iter().filter_map(|c| c.0.manifest).collect();
    let ingresses: Vec<_> = donor_sys_info.ingresses.as_ref().unwrap_or(&vec!()).iter().filter_map(|i| i.manifest.clone()).collect();

    let all_manifests: Vec<_> = [services, secrets, deployments, daemonsets, ingresses, cronjobs].concat().iter().filter_map(|i| SupportedResources::try_from(i.clone()).ok()).collect();

    let mut filtered_state = state.clone();
    filtered_state.nodes = filtered_state.nodes.into_iter().filter(|n|
        exclude_donor_node.is_none() || n.node_name != exclude_donor_node.unwrap()
    ).collect();
    
    println!("rescheduling {} resources across {} nodes", all_manifests.len(), filtered_state.nodes.len());

    let scheduler = DefaultScheduler {};

    scheduler.schedule(all_conns, &mut filtered_state, all_manifests, dry_run).await?;

    Ok(())
}
