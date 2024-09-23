use crate::config::Config;
use crate::skate::ConfigFileArgs;
use clap::{Args, Subcommand};
use std::error::Error;
use anyhow::anyhow;
use crate::ssh;
use crate::ssh::node_connection;

#[derive(Clone, Debug, Args)]
pub struct CordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}


pub async fn cordon(args: CordonArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let node = cluster.nodes.iter().find(|n| n.name == args.node).ok_or("node not found")?;

    let conn = node_connection(&cluster, &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;


    conn.execute_stdout("sudo skatelet cordon", false, false).await
}

#[derive(Clone, Debug, Args)]
pub struct UncordonArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    node: String,
}

pub async fn uncordon(args: UncordonArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let node = cluster.nodes.iter().find(|n| n.name == args.node).ok_or("node not found")?;

    let conn = node_connection(&cluster, &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;


    conn.execute_stdout("sudo skatelet uncordon", false, false).await
}
