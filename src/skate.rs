#![allow(unused)]

use std::error::Error;
use clap::{Args, Parser, Subcommand};
use k8s_openapi::Metadata;
use serde::{Deserialize, Serialize};
use crate::apply::{apply, ApplyArgs};
use crate::refresh::{refresh, RefreshArgs};
use strum_macros::{Display, EnumString};
use std::{fs, io, process};
use std::fmt::{Display, Formatter};
use std::io::Read;
use anyhow::anyhow;
use serde_yaml::Value;
use crate::config;
use crate::cluster::{cluster, ClusterArgs};
use crate::config_cmd::ConfigArgs;
use crate::cordon::{cordon, uncordon, CordonArgs, UncordonArgs};
use crate::create::{create, CreateArgs};
use crate::delete::{delete, DeleteArgs};
use crate::get::{get, GetArgs};
use crate::describe::{describe, DescribeArgs};
use crate::errors::SkateError;
use crate::logs::{logs, LogArgs};
use crate::resource::SupportedResources;
use crate::rollout::{rollout, RolloutArgs};
use crate::skate::Distribution::{Debian, Raspbian, Ubuntu, Unknown};
use crate::util::TARGET;


#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(long_about="Create resources")]
    Create(CreateArgs),
    #[command(long_about="Delete resources")]
    Delete(DeleteArgs),
    #[command(long_about="Apply kubernetes manifest files")]
    Apply(ApplyArgs),
    #[command(long_about="Refresh cluster state")]
    Refresh(RefreshArgs),
    #[command(long_about="List resources")]
    Get(GetArgs),
    #[command(long_about="View a resource")]
    Describe(DescribeArgs),
    #[command(long_about="View resource logs")]
    Logs(LogArgs),
    #[command(long_about="Configuration actions")]
    Config(ConfigArgs),
    #[command(long_about="Taint a node as unschedulable")]
    Cordon(CordonArgs),
    #[command(long_about="Remove unschedulable taint on a node")]
    Uncordon(UncordonArgs),
    #[command(long_about="Cluster actions")]
    Cluster(ClusterArgs),
    #[command(long_about="Rollout actions")]
    Rollout(RolloutArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigFileArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    pub skateconfig: String,
    #[arg(long, long_help = "Name of the context to use.")]
    pub context: Option<String>,
}

pub async fn skate() -> Result<(), SkateError> {
    config::ensure_config();
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create(args).await,
        Commands::Delete(args) => delete(args).await,

        Commands::Apply(args) => apply(args).await,
        Commands::Refresh(args) => refresh(args).await,
        Commands::Get(args) => get(args).await,
        Commands::Describe(args) => describe(args).await,
        Commands::Logs(args) => logs(args).await,
        Commands::Config(args) => crate::config_cmd::config(args),
        Commands::Cordon(args) => cordon(args).await,
        Commands::Uncordon(args) => uncordon(args).await,
        Commands::Cluster(args) => cluster(args).await,
        Commands::Rollout(args) => rollout(args).await,
    }?;
    Ok(())
    
}


pub fn read_manifests(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let api_version_key = Value::String("apiVersion".to_owned());
    let kind_key = Value::String("kind".to_owned());

    let mut result: Vec<SupportedResources> = Vec::new();

    let num_filenames = filenames.len();

    for filename in filenames {
            let str_file = {
                if num_filenames == 1 && filename == "-" {
                    let mut stdin = io::stdin();
                    let mut buffer = String::new();
                    stdin.read_to_string(&mut buffer)?;
                    buffer
                } else {
                    fs::read_to_string(filename).expect("failed to read file")
                }
            };
            for document in serde_yaml::Deserializer::from_str(&str_file) {
                let value = Value::deserialize(document).expect("failed to read document");
                if let Value::Mapping(mapping) = &value {
                    result.push(SupportedResources::try_from(&value)?)
                }
            }
        };
    Ok(result)
}

#[derive(Debug, Display, EnumString, Clone, Serialize, Deserialize)]
pub enum Os {
    Unknown,
    Linux,
    Darwin,
}

impl Os {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase() {
            s if s.contains("linux") => Os::Linux,
            s if s.contains("darwin") => Os::Darwin,
            _ => Os::Unknown
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub arch: String,
    pub distribution: Distribution,
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("arch: {}, distribution: {}", self.arch, self.distribution))
    }
}

impl Platform {
    pub fn target() -> Self {
        let parts: Vec<&str> = TARGET.split('-').collect();


        let arch = parts.first().expect("failed to find arch");

        let issue = fs::read_to_string("/etc/issue").expect("failed to read /etc/issue");
        let distro = issue.split_whitespace().next().expect("no distribution found in /etc/issue");

        Platform { arch: arch.to_string(), distribution: Distribution::from(distro) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Display)]
pub enum Distribution {
    Unknown,
    Debian,
    Raspbian,
    Ubuntu,
}

impl From<&str> for Distribution {
    fn from(s: &str) -> Self {
        match s.to_lowercase() {
            s if s.starts_with("debian") => Debian,
            s if s.starts_with("raspbian") => Raspbian,
            s if s.starts_with("ubuntu") => Ubuntu,
            _ => Unknown
        }
    }
}


pub(crate) fn exec_cmd(command: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = process::Command::new(command)
        .args(args)
        .output().map_err(|e| anyhow!(e).context("failed to run command"))?;
    if !output.status.success() {
        return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).context(format!("{} {} failed", command, args.join(" "))).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}

pub(crate) fn exec_cmd_stdout(command: &str, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = process::Command::new(command)
        .args(args)
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::inherit())
        .status().map_err(|e| anyhow!(e).context("failed to run command"))?;
    if !output.success() {
        return Err(anyhow!("exit code {}", output).context(format!("{} {} failed", command, args.join(" "))).into());
    }

    Ok(())
}



