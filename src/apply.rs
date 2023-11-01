use std::error::Error;
use clap::Args;
use itertools::{Either, Itertools};
use crate::scheduler::{CandidateNode, DefaultScheduler, Scheduler};
use crate::skate::ConfigFileArgs;
use crate::ssh;


#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    grace_period: i32,
    #[command(flatten)]
    hosts: ConfigFileArgs,
}

pub async fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let config = crate::skate::read_config(args.hosts.skateconfig)?;
    let objects = crate::skate::read_manifests(args.filename)?; // huge
    let (conns, errors) = ssh::connections(config.current_cluster()?).await;
    if errors.is_some() {
        eprintln!("{}", errors.ok_or("")?)
    }

    if conns.is_none() {
        return Ok(());
    }
    let conns = conns.ok_or("no clients")?;

    let host_infos = conns.get_hosts_info().await;

    let (candidate_nodes, errors): (Vec<_>, Vec<_>) = host_infos.into_iter().partition_map(|i| match i {
        Ok(info) => Either::Left(CandidateNode { info: info.clone(), node: config.current_cluster()
            .expect("no cluster").nodes.iter()
            .find(|n| n.name == info.node_name).expect("no node").clone() }),
        Err(err) => Either::Right(err)
    });

    if errors.len() > 0 {
        for err in errors {
            eprintln!("{}", err)
        }
    }

    let scheduler = DefaultScheduler {};
    let result = scheduler.schedule(candidate_nodes, objects).await?;
    println!("{:?}", result);
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}
