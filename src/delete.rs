use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use crate::config::Config;

use crate::skate::{ConfigFileArgs, ResourceType};
use crate::ssh;

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
}

#[derive(Debug, Args)]
pub struct DeleteResourceArgs {
    #[arg(long, long_help = "Name of the resource.")]
    name: String,
    #[arg(long, long_help = "Namespace of the resource.")]
    namespace: String,
    #[command(flatten)]
    config: ConfigFileArgs,

}


pub async fn delete(args: DeleteArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        DeleteCommands::Node(args) => delete_node(args).await?,
        DeleteCommands::Ingress(args) => delete_resource(ResourceType::Ingress, args).await?,
        DeleteCommands::Cronjob(args) => delete_resource(ResourceType::CronJob, args).await?,
        DeleteCommands::Secret(args) => delete_resource(ResourceType::Secret, args).await?,
    }
    Ok(())
}

async fn delete_resource(r_type: ResourceType, args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    // fetch state for resource type from nodes

    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
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
                results.push(result)
            }
            Err(e) => errors.push(e.to_string())
        }
    }

    match errors.is_empty() {
        false => Err(anyhow!("\n{}", errors.join("\n")).into()),
        true => Ok(())
    }
}

async fn delete_node(args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let context = match args.config.context {
        None => match config.current_context {
            None => {
                Err(anyhow!("--context is required unless there is already a current context"))
            }
            Some(ref context) => Ok(context)
        }
        Some(ref context) => Ok(context)
    }.map_err(Into::<Box<dyn Error>>::into)?;

    let (cluster_index, cluster) = config.clusters.iter().find_position(|c| c.name == context.clone()).ok_or(anyhow!("no cluster by name of {}", context))?;

    let find_result = cluster.nodes.iter().find_position(|n| n.name == args.name);

    match find_result {
        Some((p, _)) => {
            config.clusters[cluster_index].nodes.remove(p);
            config.persist(Some(args.config.skateconfig))
        }
        None => {
            Ok(())
        }
    }
}