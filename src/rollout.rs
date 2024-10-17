use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh::cluster_connections;
use clap::{Args, Subcommand};
use crate::errors::SkateError;
use crate::refresh::refreshed_state;
#[derive(Debug, Args)]
pub struct RolloutArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(
        long_about = "Resource rollout will be restarted"
    )]
    Restart(RestartArgs),
}

pub async fn rollout(global_args: RolloutArgs) -> Result<(), SkateError> {
    match global_args.command {
        Commands::Restart(args) => {
            let mut args = args;
            args.config = global_args.config;
            restart(args).await
        }
    }
}

#[derive(Debug, Args)]
pub struct RestartArgs {
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[arg(long, long_help = "Will not affect the cluster if set to true")]
    pub dry_run: bool,
    pub resource: String,
}

pub async fn restart(args: RestartArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let (conns, _) = cluster_connections(&cluster).await;

    let conns = conns.ok_or("failed to get cluster connections".to_string())?;

    let mut state = refreshed_state(&cluster.name, &conns, &config).await?;

    Ok(())
}

