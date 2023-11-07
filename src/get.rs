use std::collections::HashMap;
use std::error::Error;
use chrono::format::Fixed::RFC3339;
use chrono::{DateTime, Local, SecondsFormat};
use clap::{Args, Subcommand};
use itertools::{Either, Itertools};
use crate::config::Config;
use crate::refresh::refreshed_state;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::ConfigFileArgs;
use crate::skatelet::PodmanPodInfo;
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
pub struct GetObjectArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[command(subcommand)]
    id: Option<IdCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum GetCommands {
    Pod(GetObjectArgs),
    Deployment(GetObjectArgs),
}

pub async fn get(args: GetArgs) -> Result<(), Box<dyn Error>> {
    let global_args = args.clone();
    match args.commands {
        GetCommands::Pod(p_args) => get_pod(global_args, p_args).await,
        GetCommands::Deployment(d_args) => get_deployment(global_args, d_args).await
    }
}

async fn get_pod(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
    if errors.is_some() {
        eprintln!("{}", errors.unwrap())
    }

    if conns.is_none() {
        return Ok(());
    }

    let conns = conns.unwrap();

    let state = refreshed_state(&config.current_context.clone().unwrap_or("".to_string()), &conns, &config).await?;

    let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
        n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().find(|p| {
            let ns = args.namespace.clone().unwrap_or_default();
            let id = match args.id.clone() {
                Some(cmd) => match cmd {
                    IdCommand::Id(ids) => ids.into_iter().next().unwrap_or("".to_string())
                }
                None => "".to_string()
            };

            return (!ns.is_empty() && p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone() == ns)
                || (!id.is_empty() && (p.id == id || p.name == id));
        })
    }).collect();


    println!(
        "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
        "NAME", "READY", "STATUS", "RESTARTS", "CREATED"
    );
    for pod in pods {
        let num_containers = pod.containers.len();
        let healthy_containers = pod.containers.iter().filter(|c| {
            match c.status.as_str() {
                "running" => true,
                _ => false
            }
        }).collect::<Vec<_>>().len();
        let restarts = pod.containers.iter().map(|c| c.restart_count)
            .reduce(|a, c| a + c).unwrap_or_default();
        println!(
            "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
            pod.name, format!("{}/{}", healthy_containers, num_containers), pod.status, restarts, pod.created.to_rfc3339_opts(SecondsFormat::Secs, true)
        )
    }

    Ok(())
}

async fn get_deployment(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
    if errors.is_some() {
        eprintln!("{}", errors.unwrap())
    }

    if conns.is_none() {
        return Ok(());
    }

    let conns = conns.unwrap();

    let state = refreshed_state(&config.current_context.clone().unwrap_or("".to_string()), &conns, &config).await?;

    let pods: Vec<_> = state.nodes.iter().filter_map(|n| {
        let items: Vec<_> = n.host_info.clone()?.system_info?.pods.unwrap_or_default().into_iter().filter_map(|p| {
            let ns = args.namespace.clone();
            let id = match args.id.clone() {
                Some(cmd) => match cmd {
                    IdCommand::Id(ids) => Some(ids.into_iter().next().unwrap_or("".to_string()))
                }
                None => None
            };
            let deployment = p.labels.get("skate.io/deployment");
            match deployment {
                Some(deployment) => {
                    let match_ns = match ns {
                        Some(ns) => {
                            ns == p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone()
                        }
                        None => false
                    };
                    let match_id = match id {
                        Some(id) => {
                            id == deployment.clone()
                        }
                        None => false
                    };
                    if match_ns || match_id {
                        return Some((deployment.clone(), p));
                    }
                    None
                }
                None => None
            }
        }).collect();
        match items.len() {
            0 => None,
            _ => Some(items)
        }
    }).flatten().collect();


    println!(
        "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
        "NAME", "READY", "STATUS", "RESTARTS", "CREATED"
    );
    let pods = pods.into_iter().fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (depl, pod)| {
        acc.entry(depl).or_insert(vec![]).push(pod);
        acc
    });

    for (deployment, pods) in pods {
        let health_pods = pods.iter().filter(|p| p.status == "Running").collect_vec().len();
        let all_pods = pods.len();
        let created = pods.iter().fold(Local::now(), |acc, item| {
            if item.created < acc {
                return item.created;
            }
            return acc;
        });

        println!(
            "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
            deployment, format!("{}/{}", health_pods, all_pods), "", "", created.to_rfc3339_opts(SecondsFormat::Secs, true)
        )
    }

    Ok(())
}
