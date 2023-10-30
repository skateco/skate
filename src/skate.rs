#![allow(unused)]

use std::error::Error;
use async_trait::async_trait;
use clap::{Args, Command, Parser, Subcommand};
use k8s_openapi::{List, NamespaceResourceScope, Resource, ResourceScope};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use serde_yaml;
use serde::Deserialize;
use tokio;
use crate::apply::{apply, ApplyArgs};
use crate::on::{on, OnArgs};
use async_ssh2_tokio::client::{AuthMethod, Client, CommandExecutedResult, ServerCheckMethod};
use async_ssh2_tokio::Error as SshError;
use strum_macros::EnumString;
use std::fs;
use crate::skate::Distribution::{Debian, Raspbian, Unknown};
use crate::skate::Os::{Darwin, Linux};
use crate::ssh_client::SshClient;

const TARGET: &str = include_str!(concat!(env!("OUT_DIR"), "/../output"));

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true, version)]
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
pub struct NodeFileArgs {
    #[arg(env = "SKATE_NODES_FILE", long, long_help = "The files that contain the list of nodes.", default_value = "./.nodes.yaml")]
    pub nodes_file: String,
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
pub struct Node {
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub key: Option<String>,
}

impl Node {
    pub async fn connect(&self) -> Result<SshClient, SshError> {
        let default_key = "";
        let key = self.key.clone().unwrap_or(default_key.to_string());

        let auth_method = AuthMethod::with_key_file(key.clone().as_str(), None);
        let ssh_client = Client::connect(
            (&*self.host, self.port.unwrap_or(22)),
            self.user.clone().unwrap_or(String::from("")).as_str(),
            auth_method,
            ServerCheckMethod::NoCheck,
        ).await.expect("failed to connect");

        Ok(SshClient { client: ssh_client })
    }
}

#[derive(Deserialize)]
pub struct Nodes {
    pub user: Option<String>,
    pub key: Option<String>,
    pub nodes: Vec<Node>,
}


pub enum SupportedResources {
    Pod(Pod),
    Deployment(Deployment),
}

pub fn read_nodes(nodes_file: String) -> Result<Nodes, Box<dyn Error>> {
    let f = std::fs::File::open(nodes_file)?;
    let data: Nodes = serde_yaml::from_reader(f)?;
    let hosts: Vec<Node> = data.nodes.into_iter().map(|h| Node {
        host: h.host,
        port: h.port,
        user: h.user.or(data.user.clone()),
        key: h.key.or(data.key.clone()),
    }).collect();

    Ok(Nodes {
        user: data.user,
        key: data.key,
        nodes: hosts,
    })
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

#[derive(Debug, EnumString, Clone)]
pub enum Os {
    Unknown,
    Linux,
    Darwin,
}

#[derive(Debug, Clone)]
pub struct Platform {
    pub arch: String,
    pub os: Os,
    pub distribution: Distribution,
}

impl Platform {
    pub fn target() -> Self {
        let parts: Vec<&str> = TARGET.split('-').collect();

        let os = match parts.last().expect("failed to find os").to_lowercase() {
            s if s.starts_with("linux") => Linux,
            s if s.starts_with("darwin") => Darwin,
            _ => Os::Unknown
        };

        let arch = parts.first().expect("failed to find arch");

        let distro: Option<String> = match os {
            Linux => {
                let issue = fs::read_to_string("/etc/issue").expect("failed to read /etc/issue");
                Some(issue.split_whitespace().next().expect("no distribution found in /etc/issue").into())
            }
            _ => None
        };

        return Platform { arch: arch.to_string(), os, distribution: Distribution::from(distro.unwrap_or_default()) };
    }
}

#[derive(Debug, Clone)]
pub enum Distribution {
    Unknown,
    Debian,
    Raspbian,
}

impl From<String> for Distribution {
    fn from(s: String) -> Self {
        match s.to_lowercase() {
            s if s.starts_with("debian") => Debian,
            s if s.starts_with("raspbian") => Raspbian,
            _ => Unknown
        }
    }
}



