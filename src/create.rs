use std::error::Error;
use std::io::Write;
use std::net::ToSocketAddrs;
use anyhow::anyhow;
use base64::Engine;
use clap::{Args, Subcommand};
use itertools::Itertools;
use node::CreateNodeArgs;
use crate::config::{Cluster, Config};
use crate::skate::ConfigFileArgs;

mod node;

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
    Cluster(CreateClusterArgs),
    ClusterResources(CreateClusterResourcesArgs),
}

#[derive(Debug, Args)]
pub struct CreateClusterResourcesArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
}

#[derive(Debug, Args)]
pub struct CreateClusterArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    skateconfig: String,
    name: String,
    #[arg(long, long_help = "Default ssh user for connecting to nodes")]
    default_user: Option<String>,
    #[arg(long, long_help = "Default ssh key for connecting to nodes")]
    default_key: Option<String>,
}

pub async fn create(args: CreateArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        CreateCommands::Node(args) => node::create_node(args).await?,
        CreateCommands::ClusterResources(args) => create_cluster_resources(args).await?,
        CreateCommands::Cluster(args) => create_cluster(args).await?,
    }
    Ok(())
}

async fn create_cluster(args: CreateClusterArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.skateconfig.clone()))?;

    let cluster = Cluster {
        default_key: args.default_key,
        default_user: args.default_user,
        name: args.name.clone(),
        nodes: vec!(),
    };

    if config.clusters.iter().any(|c| c.name == args.name) {
        return Err(anyhow!("cluster by name of {} already exists in {}", args.name, args.skateconfig).into());
    }

    config.clusters.push(cluster.clone());
    config.current_context = Some(args.name.clone());

    config.persist(Some(args.skateconfig.clone()))?;

    println!("added cluster {} to {}", args.name, args.skateconfig);

    Ok(())
}

async fn create_cluster_resources(args: CreateClusterResourcesArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let context = match args.config.context {
        None => match config.current_context {
            None => {
                Err(anyhow!("--context is required unless there is already a current context"))
            }
            Some(ref context) => Ok(context)
        }
        Some(ref context) => Ok(context)
    }.map_err(Into::<Box<dyn Error>>::into)?;

    let (_, cluster) = config.clusters.iter().find_position(|c| c.name == context.clone()).ok_or(anyhow!("no cluster by name of {}", context))?;

    node::install_cluster_manifests(&args.config, cluster).await
}

