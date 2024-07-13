use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use crate::config::Config;
use crate::skate::ConfigFileArgs;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: DeleteCommands,
}

#[derive(Debug, Subcommand)]
pub enum DeleteCommands {
    Node(DeleteNodeArgs),
}

#[derive(Debug, Args)]
pub struct DeleteNodeArgs {
    #[arg(long, long_help = "Name of the node.")]
    name: String,
    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn delete(args: DeleteArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        DeleteCommands::Node(args) => delete_node(args).await.expect("failed to delete node")
    }
    Ok(())
}

async fn delete_node(args: DeleteNodeArgs) -> Result<(), Box<dyn Error>> {
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