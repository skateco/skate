#![allow(unused)]

use std::error::Error;
use clap::{Args, Command, Parser, Subcommand};
use k8s_openapi::{List, NamespaceResourceScope, Resource, ResourceScope};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::api::ApiResource;
use serde_yaml;
use serde::Deserialize;
use tokio;

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
struct HostFileArgs {
    #[arg(env = "SKATE_HOSTS_FILE", long, long_help = "The files that contain the list of hosts.", default_value = "~/.hosts.yaml")]
    hosts_file: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    grace_period: i32,
    #[command(flatten)]
    hosts: HostFileArgs,
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(apply_args) => apply(apply_args),
        _ => Ok(())
    }
}

fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let hosts = read_hosts(args.hosts.hosts_file)?;
    let merged_config = read_config(args.filename)?; // huge
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}

#[derive(Deserialize)]
struct Host {
    host: String,
}

#[derive(Deserialize)]
struct Hosts {
    hosts: Vec<Host>,
}


fn read_hosts(hosts_file: String) -> Result<Hosts, Box<dyn Error>> {
    let f = std::fs::File::open(".hosts.yaml")?;
    let data: Hosts = serde_yaml::from_reader(f)?;
    Ok(data)
}

enum SupportedResources {
    Pod(Pod),
    Deployment(Deployment)
}

fn read_config(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let api_version_key = serde_yaml::Value::String("apiVersion".to_owned());
    let kind_key = serde_yaml::Value::String("kind".to_owned());

    let mut result: Vec<SupportedResources> = Vec::new();

    for filename in  filenames {
        let file = std::fs::File::open(filename)?;
        let file: serde_yaml::Sequence = serde_yaml::from_reader(file)?;
        for document in file {
            if let serde_yaml::Value::Mapping(mapping) = &document {
                let api_version = mapping.get(&api_version_key).and_then(serde_yaml::Value::as_str);
                let kind = mapping.get(&kind_key).and_then(serde_yaml::Value::as_str);
                match (api_version, kind) {
                    (Some(api_version), Some(kind)) if
                    api_version == <k8s_openapi::api::core::v1::Pod as k8s_openapi::Resource>::API_VERSION &&
                        kind == <k8s_openapi::api::core::v1::Pod as k8s_openapi::Resource>::KIND =>
                        {
                            let pod: k8s_openapi::api::core::v1::Pod = serde::Deserialize::deserialize(document)?;
                            result.push(SupportedResources::Pod(pod))
                        }

                    (Some(api_version), Some(kind)) if
                    api_version == <k8s_openapi::api::apps::v1::Deployment as k8s_openapi::Resource>::API_VERSION &&
                        kind == <k8s_openapi::api::apps::v1::Deployment as k8s_openapi::Resource>::KIND =>
                        {
                            let deployment: k8s_openapi::api::apps::v1::Deployment = serde::Deserialize::deserialize(document)?;
                            result.push(SupportedResources::Deployment(deployment))
                        }
                    _ => {}
                }
            }
        }
    }
    Ok(result)
}