use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct NodeShellArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg()]
    node_name: String,
    #[arg(allow_hyphen_values = true, last = true)]
    cmd: Vec<String>,
}

pub trait NodeShellDeps: With<dyn SshManager> {}

pub struct NodeShell<D: NodeShellDeps> {
    pub deps: D,
}

impl<D: NodeShellDeps> NodeShell<D> {
    pub async fn node_shell(&self, args: NodeShellArgs) -> Result<(), SkateError> {
        let ssh_mgr = self.deps.get();
        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let cluster = config.active_cluster(args.config.context.clone())?;
        let node = cluster
            .nodes
            .iter()
            .find(|n| n.name == args.node_name)
            .ok_or("failed to find node".to_string())?;
        let conn = ssh_mgr.node_connect(cluster, node).await?;
        conn.execute_stdout(&args.cmd.join(" "), false, false)
            .await?;
        Ok(())
    }
}
