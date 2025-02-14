use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use anyhow::anyhow;
use clap::Args;
use std::error::Error;

#[derive(Clone, Debug, Args)]
pub struct CordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}

#[derive(Clone, Debug, Args)]
pub struct UncordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}

pub trait CordonDeps: With<dyn SshManager> {}

pub struct Cordon<D: CordonDeps> {
    pub deps: D,
}

impl<D: CordonDeps> Cordon<D> {
    pub async fn cordon(&self, args: CordonArgs) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;

        let cluster = config.active_cluster(config.current_context.clone())?;

        let node = cluster
            .nodes
            .iter()
            .find(|n| n.name == args.node)
            .ok_or("node not found".to_string())?;

        let mgr = self.deps.get();
        let conn = mgr.node_connect(cluster, node).await?;

        conn.execute_stdout("sudo skatelet cordon", false, false)
            .await?;
        Ok(())
    }

    pub async fn uncordon(&self, args: UncordonArgs) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;

        let cluster = config.active_cluster(config.current_context.clone())?;

        let node = cluster
            .nodes
            .iter()
            .find(|n| n.name == args.node)
            .ok_or("node not found".to_string())?;

        let mgr = self.deps.get();
        let conn = mgr
            .node_connect(cluster, node)
            .await
            .map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;

        conn.execute_stdout("sudo skatelet uncordon", false, false)
            .await?;
        Ok(())
    }
}
