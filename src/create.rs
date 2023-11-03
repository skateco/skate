use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use k8s_openapi::chrono::format::Pad;
use crate::config::{Config, Node};
use crate::skate::ConfigFileArgs;

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: CreateCommands,
}

#[derive(Debug, Subcommand)]
pub enum CreateCommands {
    Node(CreateNodeArgs),
}

#[derive(Debug, Args)]
pub struct CreateNodeArgs {
    #[arg(long, long_help = "Name of the node.")]
    name: String,
    #[arg(long, long_help = "IP or domain name of the node.")]
    host: String,
    #[arg(long, long_help = "Ssh user for connecting")]
    user: Option<String>,
    #[arg(long, long_help = "Ssh key for connecting")]
    key: Option<String>,
    #[arg(long, long_help = "Ssh port for connecting")]
    port: Option<u16>,

    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn create(args: CreateArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        CreateCommands::Node(args) => create_node(args).await.expect("failed to create node")
    }
    Ok(())
}

async fn create_node(args: CreateNodeArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let context = match args.config.context {
        None => match config.current_context {
            None => {
                Err(anyhow!("--cluster is required unless there is already a current context"))
            }
            Some(ref context) => Ok(context)
        }
        Some(ref context) => Ok(context)
    }.map_err(Into::<Box<dyn Error>>::into)?;

    let (cluster_index, cluster) = config.clusters.iter().find_position(|c| c.name == context.clone()).ok_or(anyhow!("no cluster by name of {}", context))?;
    let mut nodes_iter = cluster.nodes.iter();

    if nodes_iter.any(|n| n.name == args.name) {
        return Err(anyhow!("a node with the name {} already exists in the cluster", args.name).into());
    }

    if nodes_iter.any(|n| n.host == args.host) {
        return Err(anyhow!("a node with the host {} already exists in the cluster", args.host).into());
    } else {}

    config.clusters[cluster_index].nodes.push(Node {
        name: args.name,
        host: args.host,
        port: args.port,
        user: args.user,
        key: args.key,
    });
    config.persist(Some(args.config.skateconfig))?;


    Ok(())
}