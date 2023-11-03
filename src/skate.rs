#![allow(unused)]

use std::error::Error;
use async_trait::async_trait;
use clap::{Args, Command, Parser, Subcommand};
use k8s_openapi::{List, NamespaceResourceScope, Resource, ResourceScope};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use serde_yaml;
use serde::{Deserialize, Serialize};
use tokio;
use crate::apply::{apply, ApplyArgs};
use crate::refresh::{refresh, RefreshArgs};
use async_ssh2_tokio::client::{AuthMethod, Client, CommandExecutedResult, ServerCheckMethod};
use async_ssh2_tokio::Error as SshError;
use strum_macros::EnumString;
use std::{fs, process};
use std::env::var;
use std::fs::{create_dir, File};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime};
use path_absolutize::*;
use anyhow::anyhow;
use serde_yaml::Value;
use crate::config;
use crate::config::{cache_dir, Config, Node};
use crate::create::{create, CreateArgs};
use crate::delete::{delete, DeleteArgs};
use crate::skate::Distribution::{Debian, Raspbian, Unknown};
use crate::skate::Os::{Darwin, Linux};
use crate::ssh::SshClient;
use crate::util::slugify;

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
    Create(CreateArgs),
    Delete(DeleteArgs),
    Apply(ApplyArgs),
    Refresh(RefreshArgs),
}

#[derive(Debug, Args)]
pub struct ConfigFileArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    pub skateconfig: String,
    #[arg(long, long_help = "Name of the context to use.")]
    pub context: Option<String>,
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    config::ensure_config();
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create(args).await,
        Commands::Delete(args) => delete(args).await,

        Commands::Apply(args) => apply(args).await,
        Commands::Refresh(args) => refresh(args).await,
        _ => Ok(())
    }
}


impl Node {
}


#[derive(Debug, Serialize, Deserialize)]
pub enum SupportedResources {
    Pod(Pod),
    Deployment(Deployment),
}


pub fn read_manifests(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let api_version_key = Value::String("apiVersion".to_owned());
    let kind_key = Value::String("kind".to_owned());

    let mut result: Vec<SupportedResources> = Vec::new();

    for filename in filenames {
        let str_file = fs::read_to_string(filename).expect("failed to read file");
        for document in serde_yaml::Deserializer::from_str(&str_file) {
            let value = Value::deserialize(document).expect("failed to read document");
            if let Value::Mapping(mapping) = &value {
                let api_version = mapping.get(&api_version_key).and_then(Value::as_str);
                let kind = mapping.get(&kind_key).and_then(Value::as_str);
                match (api_version, kind) {
                    (Some(api_version), Some(kind)) if
                    api_version == <Pod as Resource>::API_VERSION &&
                        kind == <Pod as Resource>::KIND =>
                        {
                            let pod: Pod = serde::Deserialize::deserialize(value)?;
                            result.push(SupportedResources::Pod(pod))
                        }

                    (Some(api_version), Some(kind)) if
                    api_version == <Deployment as Resource>::API_VERSION &&
                        kind == <Deployment as Resource>::KIND =>
                        {
                            let deployment: Deployment = serde::Deserialize::deserialize(value)?;
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


pub(crate) fn exec_cmd(command: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = process::Command::new(command)
        .args(args)
        .output()
        .expect("failed to run command");
    if !output.status.success() {
        return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}


#[derive(Serialize, Deserialize)]
pub enum NodeStatus {
    Healthy,
    Unhealthy,
}

#[derive(Serialize, Deserialize)]
pub struct NodeState {
    pub node_name: String,
    pub status: NodeStatus,
    pub inventory_found: bool,
    pub inventory: Vec<SupportedResources>,
}

#[derive(Serialize, Deserialize)]
pub struct State {
    pub cluster_name: String,
    pub hash: String,
    pub nodes: Vec<NodeState>,
}

impl State {
    fn path(cluster_name: &str) -> String {
        format!("{}/{}.state", cache_dir(), slugify(cluster_name))
    }
    pub fn persist(&self) -> Result<(), Box<dyn Error>> {
        let state_file = File::create(Path::new(State::path(&self.cluster_name.clone()).as_str())).expect("unable to open state file");
        Ok(serde_json::to_writer(state_file, self).expect("failed to write json state"))
    }

    pub fn load(cluster_name: &str) -> Self {
        let file = fs::File::open(State::path(cluster_name))
            .expect("file should open read only");
        serde_json::from_reader(file).expect("failed to deserialize")
    }
}



