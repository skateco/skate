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
use crate::errors::SkateError;
use crate::refresh::refreshed_state;
use crate::resource::ResourceType;

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
    let resources = match resource_type {
        ResourceType::Deployment => {
            let objects = state.locate_objects(None, |si|si.deployments.clone(), name.as_deref(), Some(&args.namespace));
            let iter = objects.into_iter().unique_by(|i| i.0.name.clone());
            iter.collect_vec()
        },
        ResourceType::DaemonSet => {
            let objects = state.locate_objects(None, |si|si.daemonsets.clone(), name.as_deref(), Some(&args.namespace));
            let iter = objects.into_iter().unique_by(|i| i.0.name.clone());
            iter.collect_vec()
        },
        _ => panic!("unreachable")
    };
    if resources.len() == 0 {
        return Err("No resources found".to_string().into())
    }

    if resources.len() > 0 {

        println!("{} resources found\n", resources.len());
        for r in &resources {
            println!("{}", r.0.name)
        }
        println!("\n");
        
        
        let confirmation = Confirm::new()
            .with_prompt(format!("Are you sure you want to redeploy these {} resources?", resources.len()))
            .wait_for_newline(true)
            .interact()
            .unwrap();

        if !confirmation {
            return Ok(())
        }
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
