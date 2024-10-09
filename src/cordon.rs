use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh::node_connection;
use anyhow::anyhow;
use clap::Args;
use std::error::Error;
use crate::errors::SkateError;

#[derive(Clone, Debug, Args)]
pub struct CordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}


pub async fn cordon(args: CordonArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let node = cluster.nodes.iter().find(|n| n.name == args.node).ok_or("node not found".to_string())?;

    let conn = node_connection(cluster, node).await?;


    conn.execute_stdout("sudo skatelet cordon", false, false).await?;
    Ok(())
}

#[derive(Clone, Debug, Args)]
pub struct UncordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}

pub async fn uncordon(args: UncordonArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let node = cluster.nodes.iter().find(|n| n.name == args.node).ok_or("node not found".to_string())?;

    let conn = node_connection(cluster, node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;


    conn.execute_stdout("sudo skatelet uncordon", false, false).await?;
    Ok(())
}
