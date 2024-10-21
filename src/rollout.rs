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

    let mut state = &mut refreshed_state(&cluster.name, &conns, &config).await?;

    let mut catalogue = state.catalogue(None);
    
    let resources = catalogue.iter_mut().filter_map(|item| {
        if item.resource_type == resource_type && item.object.manifest.is_some() {
            let deserialized = match resource_type {
                ResourceType::Deployment => serde_yaml::from_value::<Deployment>(item.object.manifest.clone().unwrap()).ok().map(|d| SupportedResources::Deployment(d)),
                ResourceType::DaemonSet => serde_yaml::from_value::<DaemonSet>(item.object.manifest.clone().unwrap()).ok().map(|d| SupportedResources::DaemonSet(d)),
                _ => panic!("unreachable")
            };
            // invalidate current state hash
            item.object.manifest_hash = "".to_string();
            // invalidate current state hash
            item.object.manifest = item.object.manifest.clone().and_then(|m| Some(taint_manifest(m)));
            deserialized
        } else {
            None
        }
    }).collect_vec();
    
    let names = resources.iter().map(|s| s.name().clone()).collect_vec();
    
    taint_pods(
        state,
        |o| {
            NamespacedName{
                name: o.labels.get(&format!("skate.io/{}" , resource_type.to_string().to_ascii_lowercase())).unwrap_or(&"".to_string()).clone(),
                namespace: o.labels.get("skate.io/namespace").unwrap_or(&"".to_string()).clone(),
            }
        },
        names,
    );
    

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
