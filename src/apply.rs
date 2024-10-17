use anyhow::anyhow;
use clap::Args;

use crate::config::Config;
use crate::errors::SkateError;
use crate::refresh::refreshed_state;
use crate::resource::SupportedResources;
use crate::scheduler::{DefaultScheduler, Scheduler};

use crate::skate::ConfigFileArgs;
use crate::ssh;


#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    pub filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    pub grace_period: i32,
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[arg(long, long_help = "Will not affect the cluster if set to true")]
    pub dry_run: bool,
}

pub async fn apply(args: ApplyArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let objects = crate::skate::read_manifests(args.filename)?;
    apply_supported_resources(&config, objects, args.dry_run).await
}

pub(crate) async fn apply_supported_resources(config: &Config, resources: Vec<SupportedResources>, dry_run: bool) -> Result<(), SkateError> {
    let cluster = config.active_cluster(config.current_context.clone())?;
    let (conns, errors) = ssh::cluster_connections(cluster).await;
    if let Some(e) = errors {
        for e in e.errors {
            eprintln!("{} - {}", e.node_name, e.error)
        }
    };

    if conns.is_none() {
        return Err(anyhow!("failed to create cluster connections").into());
    };

    let objects: Vec<Result<_, _>> = resources.into_iter().map(|sr| sr.fixup()).collect();
    let objects: Vec<_> = objects.into_iter().map(|sr| sr.unwrap()).collect();

    let conns = conns.ok_or("no clients".to_string())?;

    let mut state = refreshed_state(&cluster.name, &conns, config).await.expect("failed to refresh state");

    let scheduler = DefaultScheduler {};
    match scheduler.schedule(&conns, &mut state, objects, dry_run).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}", e);
            return Err(anyhow!("failed to schedule resources").into());
        }
    }

    Ok(())
}