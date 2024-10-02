use crate::config::Config;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use std::error::Error;

use crate::skate::{ConfigFileArgs, ResourceType};
use crate::ssh;
use crate::util::CHECKBOX_EMOJI;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: DeleteCommands,
}

#[derive(Debug, Subcommand)]
pub enum DeleteCommands {
    Node(DeleteResourceArgs),
    Ingress(DeleteResourceArgs),
    Cronjob(DeleteResourceArgs),
    Secret(DeleteResourceArgs),
    Deployment(DeleteResourceArgs),
    Daemonset(DeleteResourceArgs),
    Service(DeleteResourceArgs),
    ClusterIssuer(DeleteResourceArgs),
}

#[derive(Debug, Args)]
pub struct DeleteResourceArgs {
    name: String,
    #[arg(long, short, long_help = "Namespace of the resource.")]
    namespace: String,
    #[command(flatten)]
    config: ConfigFileArgs,

}


pub async fn delete(args: DeleteArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        DeleteCommands::Node(args) => delete_node(args).await?,
        DeleteCommands::Daemonset(args) => delete_resource(ResourceType::DaemonSet, args).await?,
        DeleteCommands::Deployment(args) => delete_resource(ResourceType::Deployment, args).await?,
        DeleteCommands::Ingress(args) => delete_resource(ResourceType::Ingress, args).await?,
        DeleteCommands::Cronjob(args) => delete_resource(ResourceType::CronJob, args).await?,
        DeleteCommands::Secret(args) => delete_resource(ResourceType::Secret, args).await?,
        DeleteCommands::Service(args) => delete_resource(ResourceType::Service, args).await?,
        DeleteCommands::ClusterIssuer(args) => delete_resource(ResourceType::ClusterIssuer, args).await?,
    }
    Ok(())
}

async fn delete_resource(r_type: ResourceType, args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    // fetch state for resource type from nodes

    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.active_cluster(args.config.context)?).await;
    if errors.is_some() {
        eprintln!("{}", errors.unwrap())
    }

    if conns.is_none() {
        return Ok(());
    }

    let conns = conns.unwrap();

    let mut results = vec!();
    let mut errors = vec!();

    for conn in conns.clients {
        match conn.remove_resource(r_type.clone(), &args.name, &args.namespace).await {
            Ok(result) => {
                if !result.0.is_empty() {
                    result.0.trim().split("\n").map(|line| format!("{} - {}", conn.node_name, line)).for_each(|line| println!("{}", line))
                }
                results.push(result)
            }
            Err(e) => errors.push(e.to_string())
        }
    }

    match errors.is_empty() {
        false => Err(anyhow!("\n{}", errors.join("\n")).into()),
        true => {
            println!("{} deleted {} {}.{}", CHECKBOX_EMOJI, r_type, args.name, args.namespace);
            Ok(())
        }
    }
}

async fn delete_node(args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;


    let mut cluster = config.active_cluster(args.config.context.clone())?.clone();

    let find_result = cluster.nodes.iter().find_position(|n| n.name == args.name);

    match find_result {
        Some((p, _)) => {
            cluster.nodes.remove(p);
            config.replace_cluster(&cluster);
            config.persist(Some(args.config.skateconfig))
        }
        None => {
            Ok(())
        }
    }
}