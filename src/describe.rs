use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::refresh;
use crate::refresh::Refresh;
use crate::skate::ConfigFileArgs;
use crate::state::state::{ClusterState, NodeState};
use anyhow::anyhow;
use clap::{Args, Subcommand};
use k8s_openapi::api::core::v1::Node as K8sNode;

#[derive(Debug, Clone, Args)]
pub struct DescribeArgs {
    #[command(subcommand)]
    commands: DescribeCommands,
}

#[derive(Clone, Debug, Args)]
pub struct DescribeObjectArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    id: String,
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

pub trait Describer<T> {
    fn find(&self, filters: &DescribeObjectArgs, state: &ClusterState) -> Option<T>;
    fn print(&self, item: T);
}

struct NodeDescriber {}

impl Describer<NodeState> for NodeDescriber {
    fn find(&self, filters: &DescribeObjectArgs, state: &ClusterState) -> Option<NodeState> {
        state
            .nodes
            .iter()
            .find(|n| filters.id == n.node_name)
            .cloned()
    }

    fn print(&self, item: NodeState) {
        let k8s_node: K8sNode = (&item).into();
        println!("{}", serde_yaml::to_string(&k8s_node).unwrap());
    }
}

pub trait DescribeDeps: With<dyn SshManager> {}

pub struct Describe<D: DescribeDeps> {
    pub deps: D,
}

impl<D: DescribeDeps + refresh::RefreshDeps> Describe<D> {
    pub async fn describe(&self, args: DescribeArgs) -> Result<(), SkateError> {
        let global_args = args.clone();
        match args.commands {
            DescribeCommands::Pod(_p_args) => Ok(()),
            DescribeCommands::Deployment(_d_args) => Ok(()),
            DescribeCommands::Node(n_args) => self.describe_node(global_args, n_args).await,
        }
    }
    async fn describe_node(
        &self,
        global_args: DescribeArgs,
        args: DescribeObjectArgs,
    ) -> Result<(), SkateError> {
        let inspector = NodeDescriber {};
        self.describe_object(global_args, args, &inspector).await
    }

    async fn describe_object<T>(
        &self,
        _global_args: DescribeArgs,
        args: DescribeObjectArgs,
        inspector: &dyn Describer<T>,
    ) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let cluster = config.active_cluster(args.config.context.clone())?;
        let mgr = self.deps.get();
        let (conns, errs) = mgr.cluster_connect(cluster).await;
        if errs.is_some() && conns.as_ref().map(|c| c.clients.len()).unwrap_or(0) == 0 {
            return Err(anyhow!("failed to connect to any hosts: {}", errs.unwrap()).into());
        }

        let state = Refresh::<D>::refreshed_state(&cluster.name, &conns.unwrap(), &config).await?;

        let node = inspector.find(&args, &state);

        if let Some(node) = node {
            inspector.print(node)
        };

        if errs.is_some() {
            return Err(anyhow!(
                "failed to connect to some hosts: {}",
                errs.as_ref().unwrap()
            )
            .into());
        }
        Ok(())
    }
}
