use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use clap::{Args, Subcommand};

#[derive(Clone, Debug, Args)]
pub struct UpgradeArgs {
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    Node(NodeArgs),
}

#[derive(Clone, Debug, Args)]
pub struct NodeArgs {
    pub name: String,
}
pub trait UpgradeDeps: With<dyn SshManager> {}

pub struct Upgrade<D: UpgradeDeps> {
    pub deps: D,
}

impl<D: UpgradeDeps> Upgrade<D> {
    pub async fn upgrade(&self, args: UpgradeArgs) -> Result<(), SkateError> {
        match &(args.command) {
            Commands::Node(node_args) => self.upgrade_node(&args, node_args).await?,
        }
        Ok(())
    }

    async fn upgrade_node(
        &self,
        main_args: &UpgradeArgs,
        args: &NodeArgs,
    ) -> Result<(), SkateError> {
        let config = Config::load(Some(main_args.config.skateconfig.clone()))?;

        let cluster = config.active_cluster(main_args.config.context.clone())?;

        let ssh_mgr = self.deps.get();

        let node = cluster
            .nodes
            .iter()
            .find(|n| n.name == args.name)
            .ok_or("failed to find node".to_string())?;

        let conn = ssh_mgr.node_connect(cluster, node).await?;

        let si = conn.get_node_system_info().await?;

        conn.install_skatelet(si.platform).await?;
        Ok(())
    }
}
