#![allow(unused)]

use std::error::Error;
use clap::{Args, Command, Parser, Subcommand};
use k8s_openapi::{List, NamespaceResourceScope, Resource, ResourceScope};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use serde_yaml;
use serde::Deserialize;
use tokio;
use crate::apply::{apply, ApplyArgs};
use crate::on::{on, OnArgs};
use async_ssh2_tokio::client::{Client, AuthMethod, ServerCheckMethod, CommandExecutedResult};
use async_ssh2_tokio::Error as SshError;

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
    On(OnArgs),
}

#[derive(Debug, Args)]
pub struct HostFileArgs {
    #[arg(env = "SKATE_HOSTS_FILE", long, long_help = "The files that contain the list of hosts.", default_value = "~/.hosts.yaml")]
    pub hosts_file: String,
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(args) => apply(args),
        Commands::On(args) => on(args).await,
        _ => Ok(())
    }
}

#[derive(Deserialize)]
pub struct Host {
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub key: String,
    #[serde(skip)]
    client: Option<Client>,
}

impl Host {
    pub async fn connect(&mut self) -> Result<(), Box<dyn Error>> {
        let auth_method = AuthMethod::with_key_file(&*self.key, None);
        self.client = Some(Client::connect(
            (&*self.host, self.port.unwrap_or(22)),
            &*self.user,
            auth_method,
            ServerCheckMethod::NoCheck,
        ).await?);
        Ok(())
    }

    pub async fn execute(self, command: String) -> Result<CommandExecutedResult, SshError> {
        self.client.unwrap().execute("echo Hello SSH").await
    }
}

#[derive(Deserialize)]
pub struct Hosts {
    pub hosts: Vec<Host>,
}


pub enum SupportedResources {
    Pod(Pod),
    Deployment(Deployment),
}

pub fn read_hosts(hosts_file: String) -> Result<Hosts, Box<dyn Error>> {
    let f = std::fs::File::open(".hosts.yaml")?;
    let data: Hosts = serde_yaml::from_reader(f)?;
    Ok(data)
}


pub fn read_config(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let api_version_key = serde_yaml::Value::String("apiVersion".to_owned());
    let kind_key = serde_yaml::Value::String("kind".to_owned());

    let mut result: Vec<SupportedResources> = Vec::new();

    for filename in filenames {
        let file = std::fs::File::open(filename)?;
        let file: serde_yaml::Sequence = serde_yaml::from_reader(file)?;
        for document in file {
            if let serde_yaml::Value::Mapping(mapping) = &document {
                let api_version = mapping.get(&api_version_key).and_then(serde_yaml::Value::as_str);
                let kind = mapping.get(&kind_key).and_then(serde_yaml::Value::as_str);
                match (api_version, kind) {
                    (Some(api_version), Some(kind)) if
                    api_version == <Pod as Resource>::API_VERSION &&
                        kind == <Pod as Resource>::KIND =>
                        {
                            let pod: Pod = serde::Deserialize::deserialize(document)?;
                            result.push(SupportedResources::Pod(pod))
                        }

                    (Some(api_version), Some(kind)) if
                    api_version == <Deployment as Resource>::API_VERSION &&
                        kind == <Deployment as Resource>::KIND =>
                        {
                            let deployment: Deployment = serde::Deserialize::deserialize(document)?;
                            result.push(SupportedResources::Deployment(deployment))
                        }
                    _ => {}
                }
            }
        }
    }
    Ok(result)
}