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
use std::{fs, process};
use std::env::var;
use std::fs::create_dir;
use std::path::Path;
use std::time::Duration;
use path_absolutize::*;
use anyhow::anyhow;
use crate::config::{Config, Node};
use crate::skate::Distribution::{Debian, Raspbian, Unknown};
use crate::skate::Os::{Darwin, Linux};
use crate::ssh::SshClient;

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
pub struct ConfigFileArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    pub skateconfig: String,
}


fn ensure_config() -> Result<(), Box<dyn Error>> {
    let dot_dir = shellexpand::tilde("~/.skate").to_string();
    let path = Path::new(dot_dir.as_str());
    if !path.exists() {
        create_dir(path).expect("couldn't create skate config path")
    }
    let path = path.join("config.yaml");

    let default_config = Config {
        current_context: None,
        clusters: vec![],
    };

    if !path.exists() {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
            .expect("couldn't open config file");
        serde_yaml::to_writer(f, &default_config).unwrap();
    }

    Ok(())
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    ensure_config();
    let args = Cli::parse();
    match args.command {
        Commands::Apply(args) => apply(args).await,
        Commands::On(args) => on(args).await,
        _ => Ok(())
    }
}


impl Node {
    pub async fn connect(&self) -> Result<SshClient, Box<dyn Error>> {
        let default_key = "";
        let key = self.key.clone().unwrap_or(default_key.to_string());
        let key = shellexpand::tilde(&key);
        let timeout = Duration::from_secs(5);

        let auth_method = AuthMethod::with_key_file(&key, None);
        let result = tokio::time::timeout(timeout, Client::connect(
            (&*self.host, self.port.unwrap_or(22)),
            self.user.clone().unwrap_or(String::from("")).as_str(),
            auth_method,
            ServerCheckMethod::NoCheck,
        )).await;

        let result: Result<_, Box<dyn Error>> = match result {
            Ok(r2) => r2.map_err(|e| e.into()),
            _ => Err(anyhow!("timeout").into())
        };

        let ssh_client = result?;

        Ok(SshClient { node_name: self.name.clone(), client: ssh_client })
    }
}


#[derive(Debug)]
pub enum SupportedResources {
    Pod(Pod),
    Deployment(Deployment),
}

pub fn read_config(path: String) -> Result<Config, Box<dyn Error>> {
    let path = shellexpand::tilde(&path).to_string();
    let path = Path::new(&path);
    let f = std::fs::File::open(path)?;
    let data: Config = serde_yaml::from_reader(f)?;
    Ok(data)
}


pub fn read_manifests(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
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


pub(crate) fn exec_cmd(command: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = process::Command::new(command)
        .args(args)
        .output()
        .expect("failed to find os");
    if !output.status.success() {
        return Err(anyhow!("{}, {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}



