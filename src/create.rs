use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use crate::config::{Config, Node};
use crate::skate::ConfigFileArgs;
use crate::ssh::node_connection;

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
    let mut nodes_iter = cluster.nodes.clone().into_iter();

    let existing_node = nodes_iter.find(|n| n.name == args.name || n.host == args.host);

    let (new, node) = match existing_node {
        Some(node) => (false, node.clone()),
        None => {
            let node = Node {
                name: args.name,
                host: args.host,
                port: args.port,
                user: args.user,
                key: args.key,
            };
            config.clusters[cluster_index].nodes.push(node.clone());
            (true, node)
        }
    };

    let conn = node_connection(&config.clusters[cluster_index], &node).await.expect("failed to connect");
    let info = conn.get_host_info().await.expect("failed to get host info");
    match info.skatelet_version {
        None => {
            // install skatelet
            let _ = conn.install_skatelet(info.platform).await.expect("failed to install skatelet");
        }
        _ => {
            println!("skatelet already installed")
        }
    }

    if new {
        config.persist(Some(args.config.skateconfig))?;
    }

    Ok(())
}