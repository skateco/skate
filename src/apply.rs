use std::error::Error;
use clap::Args;

use crate::config::Config;
use crate::refresh::refreshed_state;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::ConfigFileArgs;
use crate::ssh;

use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};


#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    grace_period: i32,
    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig)).expect("failed to load skate config");
    let objects = crate::skate::read_manifests(args.filename).unwrap(); // huge
    let cluster = config.current_cluster()?;
    let (conns, errors) = ssh::cluster_connections(cluster).await;
    match errors {
        Some(e) => {
            eprintln!("{}", e)
        }
        _ => {}
    };

    match conns {
        None => {
            return Ok(());
        }
        _ => {}
    };

    let objects = objects.into_iter().map(|sr| sr.fixup()).collect();

    let conns = conns.ok_or("no clients")?;

    let mut state = refreshed_state(&cluster.name, &conns, &config).await.expect("failed to refresh state");


    let scheduler = DefaultScheduler {};
    let results = scheduler.schedule(conns, &mut state, objects).await?;


    for placement in results.placements {
        match placement.error {
            None => println!("{} resource applied {} ", placement.resource, CHECKBOX_EMOJI),
            Some(err) => eprintln!("{} resource apply failed: {} {} ", placement.resource, err, CROSS_EMOJI)
        }
    }

    state.persist()?;
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}
