use std::error::Error;
use anyhow::anyhow;
use clap::Args;

use crate::config::Config;
use crate::refresh::refreshed_state;
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
}

pub async fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig)).expect("failed to load skate config");
    let objects = crate::skate::read_manifests(args.filename).unwrap(); // huge
    let cluster = config.current_cluster()?;
    let (conns, errors) = ssh::cluster_connections(cluster).await;
    match errors {
        Some(e) => {
            for e in e.errors {
                eprintln!("{} - {}", e.node_name, e.error)
            }
        }
        _ => {}
    };

    match conns {
        None => {
            return Err(anyhow!("failed to create cluster connections").into());
        }
        _ => {}
    };

    let objects: Vec<Result<_, _>> = objects.into_iter().map(|sr| sr.fixup()).collect();
    let objects: Vec<_> = objects.into_iter().map(|sr| sr.unwrap()).collect();

    let conns = conns.ok_or("no clients")?;

    let mut state = refreshed_state(&cluster.name, &conns, &config).await.expect("failed to refresh state");

    let scheduler = DefaultScheduler {};
    match scheduler.schedule(&conns, &mut state, objects).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}", e);
            return Err(anyhow!("failed to schedule resources").into());
        }
    }

    state.persist()?;

    Ok(())
}
