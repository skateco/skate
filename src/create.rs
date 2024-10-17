use anyhow::anyhow;
use clap::{Args, Subcommand};
use node::CreateNodeArgs;
use crate::config::{Cluster, Config};
use crate::errors::SkateError;
use crate::refresh::refreshed_state;
use crate::skate::ConfigFileArgs;
use crate::skatelet::JobArgs;
use crate::ssh;

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
    Job(CreateJobArgs),
}

#[derive(Debug, Args)]
pub struct CreateClusterResourcesArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
}

#[derive(Debug, Args)]
pub struct CreateJobArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(flatten)]
    args: JobArgs,
    #[arg(long, short, long_help = "Namespace of the resource.", default_value_t = String::from("default"))]
    namespace: String,
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


pub async fn create(args: CreateArgs) -> Result<(), SkateError> {
    match args.command {
        CreateCommands::Node(args) => node::create_node(args).await?,
        CreateCommands::ClusterResources(args) => create_cluster_resources(args).await?,
        CreateCommands::Cluster(args) => create_cluster(args).await?,
        CreateCommands::Job(args) => create_job(args).await?
    }
    Ok(())
}

async fn create_cluster(args: CreateClusterArgs) -> Result<(), SkateError> {
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

async fn create_cluster_resources(args: CreateClusterResourcesArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(args.config.context.clone())?;

    node::install_cluster_manifests(&args.config, cluster).await?;
    Ok(())
}


async fn create_job(args: CreateJobArgs) -> Result<(), SkateError> {

    let (from_kind, from_name) = args.args.from.split_once("/").ok_or("invalid --from".to_string())?;
    if from_kind !="cronjob" {
        return Err("only cronjob is supported".to_string().into());
    }

    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(args.config.context)?;


    let (conns, errors) = ssh::cluster_connections(cluster).await;
    if let Some(e) = errors {
        for e in e.errors {
            eprintln!("{} - {}", e.node_name, e.error)
        }
    };

    let conns = match conns {
        None => {
            return Err(anyhow!("failed to create cluster connections").into());
        },
        Some(c) => c
    };

    let state = refreshed_state(&cluster.name, &conns, &config).await.expect("failed to refresh state");

    let cjobs = state.locate_objects(None, |si| { si.cronjobs.clone() }, Some(from_name), Some(&args.namespace));
    if cjobs.is_empty() {
        return Err(anyhow!("no cronjobs found by name of {} in namespace {}", args.args.from, args.namespace).into());
    }
    let (_info, node) = cjobs.first().unwrap();

    let conn = conns.find(&node.node_name).unwrap();

    let wait_flag = if args.args.wait { "--wait" } else { "" };

    let cmd = format!("sudo skatelet create --namespace {} job {} --from {} {}", &args.namespace, &wait_flag, &args.args.from, &args.args.name);
    conn.execute_stdout(&cmd,false, false).await?;
    Ok(())
}