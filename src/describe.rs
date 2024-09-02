use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use k8s_openapi::api::core::v1::Node as K8sNode;
use crate::config::Config;
use crate::refresh::refreshed_state;
use crate::skate::ConfigFileArgs;
use crate::ssh;
use crate::state::state::{ClusterState, NodeState};

#[derive(Debug, Clone, Args)]
pub struct DescribeArgs {
    #[command(subcommand)]
    commands: DescribeCommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IdCommand {
    #[clap(external_subcommand)]
    Id(Vec<String>)
}

#[derive(Clone, Debug, Args)]
pub struct DescribeObjectArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[command(subcommand)]
    id: Option<IdCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DescribeCommands {
    #[command(alias("pods"))]
    Pod(DescribeObjectArgs),
    #[command(alias("deployments"))]
    Deployment(DescribeObjectArgs),
    #[command(alias("nodes"))]
    Node(DescribeObjectArgs),
}

pub async fn describe(args: DescribeArgs) -> Result<(), Box<dyn Error>> {
    let global_args = args.clone();
    match args.commands {
        DescribeCommands::Pod(_p_args) => Ok(()),
        DescribeCommands::Deployment(_d_args) => Ok(()),
        DescribeCommands::Node(n_args) => describe_node(global_args, n_args).await
    }
}

pub trait Describer<T> {
    fn find(&self, filters: &DescribeObjectArgs, state: &ClusterState) -> Option<T>;
    fn print(&self, item: T);
}

struct NodeDescriber {}

impl Describer<NodeState> for NodeDescriber {
    fn find(&self, filters: &DescribeObjectArgs, state: &ClusterState) -> Option<NodeState> {
        let id = filters.id.as_ref().and_then(|cmd| match cmd {
            IdCommand::Id(ids) => ids.first().and_then(|id| Some((*id).clone())),
        });
        let id = match id {
            Some(id) => id,
            None => {
                return None;
            }
        };

        state.nodes.iter().find(|n| *id == n.node_name.clone()).and_then(|n| Some(n.clone()))
    }

    fn print(&self, item: NodeState) {
        let k8s_node: K8sNode = item.into();
        println!("{}", serde_yaml::to_string(&k8s_node).unwrap());
    }
}

async fn describe_node(global_args: DescribeArgs, args: DescribeObjectArgs) -> Result<(), Box<dyn Error>> {
    let inspector = NodeDescriber {};
    describe_object(global_args, args, &inspector).await
}

async fn describe_object<T>(_global_args: DescribeArgs, args: DescribeObjectArgs, inspector: &dyn Describer<T>) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let cluster = config.current_cluster()?;
    let (conns, errs) = ssh::cluster_connections(&cluster).await;
    if errs.is_some() {
        if conns.as_ref().and_then(|c| Some(c.clients.len())).unwrap_or(0) == 0 {
            return Err(anyhow!("failed to connect to any hosts: {}", errs.unwrap()).into());
        }
    }

    let state = refreshed_state(&cluster.name, &conns.unwrap(), &config).await?;

    let node = inspector.find(&args, &state);

    match node {
        Some(node) => inspector.print(node),
        None => {}
    };

    if errs.is_some() {
        return Err(anyhow!("failed to connect to some hosts: {}", errs.as_ref().unwrap()).into());
    }
    Ok(())
}