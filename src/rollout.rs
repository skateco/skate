use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::str::FromStr;
use anyhow::anyhow;
use async_ssh2_tokio::ToSocketAddrsWithHostname;
use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh::cluster_connections;
use clap::{Args, Subcommand};
use dialoguer::Confirm;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use serde_yaml::Value;
use crate::errors::SkateError;
use crate::filestore::ObjectListItem;
use crate::refresh::refreshed_state;
use crate::resource::{ResourceType, SupportedResources};
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skatelet::system::podman::PodmanPodInfo;
use crate::skatelet::SystemInfo;
use crate::state::state::ClusterState;
use crate::util::NamespacedName;

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
    pub resource: ResourceArg,
    #[arg(long, short, long_help = "Namespace of the resource.", default_value_t = String::from("default"))]
    namespace: String,
    #[arg(long, short, long_help = "Answer yes to confirmation")]
    pub yes: bool,
}

fn get_objects(state: &ClusterState,  selector: impl Fn(&SystemInfo) -> Option<Vec<ObjectListItem>>, name: Option<&str>, namespace: Option<&str>) -> Vec<ObjectListItem> {
    let objects = state.locate_objects(None, selector, name.as_deref(), namespace);
    objects.iter().unique_by(|i| i.0.name.clone()).flat_map(|(o, _)| {
        match &o.manifest {
            Some(m) => Some(o.clone()),
            None => None
        }
    }).collect()
}

fn taint_manifest(mut v: Value) -> Value {
    v["metadata"]["labels"]["skate.io/hash"] = Value::from("");
    v
}

fn taint_pods(mut state: &mut ClusterState, name_selector: impl Fn(&PodmanPodInfo) -> NamespacedName, names: Vec<NamespacedName>) {

    state.nodes.iter_mut().for_each(|n|
        match n.host_info.as_mut() {
            Some(hi) => {
                match hi.system_info.as_mut() {
                    Some(si) => {
                        match &mut si.pods {
                            Some(v) => {
                                v.iter_mut().for_each(|item|
                                    for name in &names {
                                        if &name_selector(item) != name {
                                            return
                                        }
                                        let mut labels=  item.labels.clone();
                                        labels.insert("skate.io/hash".to_string(), "".to_string());
                                        item.labels = labels;
                                    }
                                );
                            }
                            None => {}
                        }
                    },
                    None => {}
                };

            }
            None =>{}
        }
    )

}

fn taint_objects(mut state: &mut ClusterState, object_selector: impl Fn(&mut SystemInfo) -> Option<&mut[ObjectListItem]>, names: Vec<NamespacedName>) {

    state.nodes.iter_mut().for_each(|n|
        match n.host_info.as_mut() {
            Some(hi) => {
                match hi.system_info.as_mut() {
                    Some(si) => {
                        let objects  = object_selector(si);
                        match objects {
                            Some(v) => {
                                v.iter_mut().for_each(|item|
                                    for name in &names {
                                        if &item.name != name {
                                            return
                                        }

                                        match &item.manifest {
                                            Some(manifest) =>
                                                item.manifest = Some(taint_manifest(manifest.clone())),
                                            None => {}
                                        }

                                    }
                                );
                            }
                            None => {}
                        }
                    },
                    None => {}
                };

            }
            None =>{}
        }
    )

}

pub async fn restart(args: RestartArgs) -> Result<(), SkateError> {
    let (resource_type, name) = args.resource.parse()?;

    match resource_type {
        ResourceType::Deployment => {},
        ResourceType::DaemonSet => {},
        _ => return Err("resource type not supported".to_string().into())
    }

    let config = Config::load(Some(args.config.skateconfig.clone()))?;

    let cluster = config.active_cluster(config.current_context.clone())?;

    let (conns, _) = cluster_connections(&cluster).await;

    let conns = conns.ok_or("failed to get cluster connections".to_string())?;

    let mut state = refreshed_state(&cluster.name, &conns, &config).await?;

    // fetch and clone the existing resources as to_deploy
    // alter the hash on the original state objects
    // this will ensure a redeploy of everything since the hashes will not match
    // and next deploy will work as normal


    // ask question if all deployments or all daemonsets
    let resources: Vec<SupportedResources> = match resource_type {
        ResourceType::Deployment => {
            let objects= get_objects(&state, |si|si.deployments.clone(), name.as_deref(), Some(&args.namespace));
            let objects = objects.into_iter().map( |v| serde_yaml::from_value::<Deployment>(v.manifest.unwrap()).ok()).flatten().map(|d| SupportedResources::Deployment(d)).collect_vec();
            let names = objects.iter().map(|o| o.name()).collect_vec();
            taint_objects(
                &mut state,
                |si| si.deployments.as_deref_mut(),
                names.clone()
            );

            taint_pods(
                &mut state,
                |o| {
                    NamespacedName{
                        name: o.labels.get("skate.io/deployment").unwrap_or(&"".to_string()).clone(),
                        namespace: o.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone(),
                    }
                },
                names
            );


            objects
        },
        ResourceType::DaemonSet => {
            let objects= get_objects(&state, |si|si.daemonsets.clone(), name.as_deref(), Some(&args.namespace));
            let objects = objects.into_iter().map( |v| serde_yaml::from_value::<DaemonSet>(v.manifest.unwrap()).ok()).flatten().map(|d| SupportedResources::DaemonSet(d)).collect_vec();
            let names = objects.iter().map(|o| o.name()).collect_vec();
            taint_objects(
                &mut state,
                |si| si.daemonsets.as_deref_mut(),
                names.clone()
            );

            taint_pods(
                &mut state,
                |o| {
                    NamespacedName{
                        name: o.labels.get("skate.io/daemonset").unwrap_or(&"".to_string()).clone(),
                        namespace: o.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone(),
                    }
                },
                names
            );
            objects
        },
        _ => panic!("unreachable")
    };
    if resources.len() == 0 {
        return Err("No resources found".to_string().into())
    }

    if resources.len() > 0 {

        println!("{} {} resources found\n", resources.len(), resource_type);
        for r in &resources {
            println!("{}", r.name())
        }


        if !args.dry_run && ! args.yes{

            let confirmation = Confirm::new()
                .with_prompt(format!("Are you sure you want to redeploy these {} resources?", resources.len()))
                .wait_for_newline(true)
                .interact()
                .unwrap();

            if !confirmation {
                return Ok(())
            }

        }

        let scheduler = DefaultScheduler{};

        let _ = scheduler.schedule(&conns, &mut state, resources, args.dry_run).await?;
    }


    Ok(())
}

#[derive( Clone, Debug)]
pub struct ResourceArg(String);

impl From<OsString> for ResourceArg {
    fn from(value: OsString) -> Self {
        ResourceArg(value.into_string().unwrap())
    }
}

impl ResourceArg {
    pub fn parse(&self) -> Result<(ResourceType, Option<String>), Box<dyn Error>> {
        let parts = self.0.splitn(2, "/").collect_vec();
        if parts.len() == 0 || parts.len() > 2 {
            return Err(anyhow!("invalid resource format").into())
        }

        let resource= parts[0];

        let name = if parts.len() == 2 {
            parts.last().and_then(|s| Some(s.to_string()))
        }else { None};


        let resource = ResourceType::from_str(&resource)?;

        Ok((resource, name))
    }
}
