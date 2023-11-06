use std::error::Error;
use clap::Args;
use itertools::{Either, Itertools};
use crate::config::Config;
use crate::scheduler::{CandidateNode, DefaultScheduler, Scheduler};
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::ConfigFileArgs;
use crate::ssh;
use crate::state::state::State;
use crate::util::CHECKBOX_CHAR;


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

    let conns = conns.ok_or("no clients")?;

    let host_infos = conns.get_hosts_info().await;
    let healthy_host_infos: Vec<_> = host_infos.iter().filter_map(|h| match h {
        Ok(r) => Some((*r).clone()),
        Err(_) => None,
    }).collect();

    let (candidate_nodes, errors): (Vec<_>, Vec<_>) = host_infos.into_iter().partition_map(|i| match i {
        Ok(info) => {
            let node = config.current_cluster()
                .expect("no cluster").nodes.iter()
                .find(|n| n.name == info.node_name).expect("no node").clone();

            Either::Left(CandidateNode {
                info: info.clone(),
                node,
            })
        }
        Err(err) => Either::Right(err)
    });

    if errors.len() > 0 {
        for err in errors {
            eprintln!("{}", err)
        }
    }

    let mut state = match State::load(&cluster.name) {
        Ok(state) => state,
        Err(_) => State {
            cluster_name: cluster.name.clone(),
            hash: "".to_string(),
            nodes: vec![],
            orphaned_nodes: None,
        }
    };

    let recon_result = state.reconcile(&config, &healthy_host_infos)?;

    let scheduler = DefaultScheduler {};
    let results = scheduler.schedule(conns, &mut state, candidate_nodes, objects).await?;


    let mut should_err = false;
    for result in results {
        match result.status {
            Scheduled(message) => println!("{} {} resource applied ({})", CHECKBOX_CHAR, result.object, message),
            ScheduleError(err) => eprintln!("{} resource apply failed: {}", result.object, err)
        }
    }

    state.persist()?;
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}
