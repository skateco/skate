use std::collections::HashMap;
use std::error::Error;

use chrono::{Local, SecondsFormat};
use clap::{Args, Subcommand};
use itertools::{Itertools};
use crate::config::Config;
use crate::refresh::refreshed_state;


use crate::skate::ConfigFileArgs;
use crate::skatelet::{PodmanPodInfo, PodmanPodStatus};
use crate::ssh;
use crate::state::state::{ClusterState, NodeState};


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
    #[command(alias("pods"))]
    Pod(GetObjectArgs),
    #[command(alias("deployments"))]
    Deployment(GetObjectArgs),
    #[command(alias("nodes"))]
    Node(GetObjectArgs),
}

pub async fn get(args: GetArgs) -> Result<(), Box<dyn Error>> {
    let global_args = args.clone();
    match args.commands {
        GetCommands::Pod(p_args) => get_pod(global_args, p_args).await,
        GetCommands::Deployment(d_args) => get_deployment(global_args, d_args).await,
        GetCommands::Node(n_args) => get_nodes(global_args, n_args).await
    }
}

pub trait Lister<T> {
    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<T>;
    fn print(&self, items: Vec<T>);
}

async fn get_objects<T>(_global_args: GetArgs, args: GetObjectArgs, lister: &dyn Lister<T>) -> Result<(), Box<dyn Error>> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;
    if errors.is_some() {
        eprintln!("{}", errors.unwrap())
    }

    if conns.is_none() {
        return Ok(());
    }

    let conns = conns.unwrap();

    let state = refreshed_state(&config.current_context.clone().unwrap_or("".to_string()), &conns, &config).await?;

    let objects = lister.list(&args, &state);

    lister.print(objects);
    Ok(())
}

struct PodLister {}

impl Lister<PodmanPodInfo> for PodLister {
    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<PodmanPodInfo> {
        let ns = filters.namespace.clone().unwrap_or_default();
        let id = match filters.id.clone() {
            Some(cmd) => match cmd {
                IdCommand::Id(ids) => ids.into_iter().next().unwrap_or("".to_string())
            }
            None => "".to_string()
        };

        let pods = state.filter_pods(&|p| {
            return (!ns.is_empty() && p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone() == ns)
                || (!id.is_empty() && (p.id == id || p.name == id)) || (ns.is_empty() && id.is_empty());
        });
        pods.iter().map(|(p, n)| p.clone()).collect()
    }

    fn print(&self, pods: Vec<PodmanPodInfo>) {
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
            let restarts = pod.containers.iter().map(|c| c.restart_count.unwrap_or_default())
                .reduce(|a, c| a + c).unwrap_or_default();
            println!(
                "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
                pod.name, format!("{}/{}", healthy_containers, num_containers), pod.status, restarts, pod.created.to_rfc3339_opts(SecondsFormat::Secs, true)
            )
        }
    }
}


async fn get_pod(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = PodLister {};
    get_objects(global_args, args, &lister).await
}

struct DeploymentLister {}

impl Lister<(String, PodmanPodInfo)> for DeploymentLister {
    fn list(&self, args: &GetObjectArgs, state: &ClusterState) -> Vec<(String, PodmanPodInfo)> {
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
                        let match_ns = match ns.clone() {
                            Some(ns) => {
                                ns == p.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone()
                            }
                            None => false
                        };
                        let match_id = match id.clone() {
                            Some(id) => {
                                id == deployment.clone()
                            }
                            None => false
                        };
                        if match_ns || match_id || (id.is_none() && ns.is_none()) {
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
        pods
    }

    fn print(&self, items: Vec<(String, PodmanPodInfo)>) {
        println!(
            "{0: <30}  {1: <10}  {2: <10}  {3: <10}  {4: <30}",
            "NAME", "READY", "STATUS", "RESTARTS", "CREATED"
        );
        let pods = items.into_iter().fold(HashMap::<String, Vec<PodmanPodInfo>>::new(), |mut acc, (depl, pod)| {
            acc.entry(depl).or_insert(vec![]).push(pod);
            acc
        });

        for (deployment, pods) in pods {
            let health_pods = pods.iter().filter(|p| PodmanPodStatus::Running == p.status).collect_vec().len();
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
    }
}

async fn get_deployment(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = DeploymentLister {};
    get_objects(global_args, args, &lister).await
}


struct NodeLister {}

impl Lister<NodeState> for NodeLister {
    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<NodeState> {
        state.nodes.iter().filter(|n| {
            match filters.clone().id {
                Some(id) => match id {
                    IdCommand::Id(ids) => {
                        ids.first().unwrap_or(&"".to_string()).clone() == n.node_name
                    }
                }
                _ => true
            }
        }).map(|n| n.clone()).collect()
    }

    fn print(&self, items: Vec<NodeState>) {
        println!(
            "{0: <30}  {1: <10}  {2: <10}",
            "NAME", "PODS", "STATUS"
        );
        for node in items {
            let num_pods = match node.host_info {
                Some(hi) => match hi.system_info {
                    Some(si) => match si.pods {
                        Some(pods) => pods.len(),
                        _ => 0
                    }
                    _ => 0
                }
                _ => 0
            };
            println!(
                "{0: <30}  {1: <10}  {2: <10}",
                node.node_name, num_pods, node.status
            )
        }
    }
}

async fn get_nodes(global_args: GetArgs, args: GetObjectArgs) -> Result<(), Box<dyn Error>> {
    let lister = NodeLister {};
    get_objects(global_args, args, &lister).await
}

