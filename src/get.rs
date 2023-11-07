use std::error::Error;
use chrono::format::Fixed::RFC3339;
use chrono::SecondsFormat;
use clap::{Args, Subcommand};
use itertools::{Either, Itertools};
use crate::config::Config;
use crate::refresh::refreshed_state;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::ConfigFileArgs;
use crate::ssh;
use crate::state::state::ClusterState;
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};


#[derive(Debug, Clone, Args)]
pub struct GetArgs {
    #[command(subcommand)]
    commands: GetCommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IdCommand {
    #[clap(external_subcommand)]
    Id(Vec<String>)
}

#[derive(Clone, Debug, Args)]
pub struct GetPodArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[command(subcommand)]
    id: IdCommand,
}

#[derive(Clone, Debug, Args)]
pub struct GetDeploymentArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[command(subcommand)]
    id: IdCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum GetCommands {
    Pod(GetPodArgs),
    Deployment(GetDeploymentArgs),
}

pub async fn get(args: GetArgs) -> Result<(), Box<dyn Error>> {
    let global_args = args.clone();
    match args.commands {
        GetCommands::Pod(p_args) => get_pod(global_args, p_args).await,
        GetCommands::Deployment(d_args) => get_deployment(global_args, d_args).await
    }
}

async fn get_pod(global_args: GetArgs, args: GetPodArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
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

    let state = refreshed_state(&config.current_context.clone().unwrap_or("".to_string()), &conns, &config).await?;

    let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
        n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().find(|p| {
            let ns = args.namespace.clone().unwrap_or_default();
            let id = match args.id.clone() {
                IdCommand::Id(ids) => ids.into_iter().next().unwrap_or("".to_string())
            };

            return (!ns.is_empty() && p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone() == ns)
                || (!id.is_empty() && (p.id == id || p.name == id));
        })
    }).collect();


    println!(
        "{0: <30} | {1: <10} | {2: <10} | {3: <10} | {4: <30}",
        "NAME", "READY", "STATUS", "RESTARTS", "CREATED"
    );
    for pod in pods {
        let restarts = pod.containers.iter().map(|c| c.restart_count)
            .reduce(|a, c| a + c).unwrap_or_default();
        println!(
            "{0: <30} | {1: <10} | {2: <10} | {3: <10} | {4: <30}",
            pod.name, "1/1", pod.status, restarts, pod.created.to_rfc3339_opts(SecondsFormat::Secs, true)
        )
    }

    Ok(())
}

async fn get_deployment(global_args: GetArgs, args: GetDeploymentArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
