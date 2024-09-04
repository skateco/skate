mod node;
mod ingress;
mod deployment;
mod cronjob;
mod pod;
mod lister;
mod daemonset;
mod secret;
mod service;

use std::error::Error;



use clap::{Args, Subcommand};

use crate::config::Config;
use crate::refresh::refreshed_state;


use crate::skate::{ConfigFileArgs};

use crate::{ssh};

use crate::get::cronjob::CronjobsLister;
use crate::get::daemonset::DaemonsetLister;
use crate::get::deployment::DeploymentLister;
use crate::get::ingress::IngresssLister;
use crate::get::lister::Lister;
use crate::get::node::NodeLister;
use crate::get::pod::PodLister;
use crate::get::secret::SecretLister;
use crate::get::service::ServiceLister;

#[derive(Debug, Clone, Args)]
pub struct GetArgs {
    #[command(subcommand)]
    commands: GetCommands,
}

#[derive(Clone, Debug, Args)]
pub struct GetObjectArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[arg()]
    id: Option<String>
}

#[derive(Clone, Debug, Subcommand)]
pub enum GetCommands {
    #[command(alias("pods"))]
    Pod(GetObjectArgs),
    #[command(alias("deployments"))]
    Deployment(GetObjectArgs),
    #[command(alias("daemonsets"))]
    Daemonset(GetObjectArgs),
    #[command(alias("nodes"))]
    Node(GetObjectArgs),
    #[command()]
    Ingress(GetObjectArgs),
    #[command(alias("cronjobs"))]
    Cronjob(GetObjectArgs),
    #[command(alias("secrets"))]
    Secret(GetObjectArgs),
    #[command(alias("services"))]
    Service(GetObjectArgs),
}

pub async fn get(args: GetArgs) -> Result<(), Box<dyn Error>> {
    let global_args = args.clone();
    match args.commands {
        GetCommands::Pod(args) => get_pod(global_args, args).await,
        GetCommands::Deployment(args) => get_deployment(global_args, args).await,
        GetCommands::Daemonset(args) => get_daemonsets(global_args, args).await,
        GetCommands::Node(args) => get_nodes(global_args, args).await,
        GetCommands::Ingress(args) => get_ingress(global_args, args).await,
        GetCommands::Cronjob(args) => get_cronjobs(global_args, args).await,
        GetCommands::Secret(args) => get_secrets(global_args, args).await,
        GetCommands::Service(args) => get_services(global_args, args).await,
    }
}


async fn get_objects<T>(_global_args: GetArgs, args: GetObjectArgs, lister: &dyn Lister<T>) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
    if errors.is_some() {
        eprintln!("{}", errors.unwrap())
    }

    if conns.is_none() {
        return Ok(());
    }

    let conns = conns.unwrap();

    let state = refreshed_state(&config.current_context.clone().unwrap_or("".to_string()), &conns, &config).await?;

    let objects = lister.list(&args, &state);

    if objects.len() == 0 {
        println!("No resources found for namespace {}", args.namespace.unwrap_or("default".to_string()));
        return Ok(());
    }

    lister.print(objects);
    Ok(())
}





async fn get_deployment(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = DeploymentLister {};
    get_objects(global_args, args, &lister).await
}

async fn get_daemonsets(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = DaemonsetLister {};
    get_objects(global_args, args, &lister).await
}

async fn get_pod(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = PodLister {};
    get_objects(global_args, args, &lister).await
}


async fn get_ingress(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = IngresssLister {};
    get_objects(global_args, args, &lister).await
}

async fn get_cronjobs(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = CronjobsLister {};
    get_objects(global_args, args, &lister).await
}


async fn get_nodes(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = NodeLister {};
    get_objects(global_args, args, &lister).await
}

async fn get_secrets(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = SecretLister{};
    get_objects(global_args, args, &lister).await
}

async fn get_services(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = ServiceLister{};
    get_objects(global_args, args, &lister).await
}


