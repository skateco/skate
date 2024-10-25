#![allow(unused)]

use std::error::Error;
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use crate::apply::{Apply, ApplyArgs, ApplyDeps};
use crate::refresh::{Refresh, RefreshArgs, RefreshDeps};
use strum_macros::{Display, EnumString};
use std::{fs, io};
use std::fmt::{Display, Formatter};
use std::io::Read;
use serde_yaml::Value;
use crate::config;
use crate::cluster::{Cluster, ClusterArgs, ClusterDeps};
use crate::config_cmd::ConfigArgs;
use crate::cordon::{Cordon, CordonArgs, CordonDeps, UncordonArgs};
use crate::create::{Create, CreateArgs, CreateDeps};
use crate::delete::{Delete, DeleteArgs, DeleteDeps};
use crate::deps::Deps;
use crate::get::{Get, GetArgs, GetDeps};
use crate::describe::{Describe, DescribeArgs, DescribeDeps};
use crate::errors::SkateError;
use crate::logs::{LogArgs, Logs, LogsDeps};
use crate::resource::SupportedResources;
use crate::rollout::{Rollout, RolloutArgs, RolloutDeps};
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
    #[command(long_about = "Create resources")]
    Create(CreateArgs),
    #[command(long_about = "Delete resources")]
    Delete(DeleteArgs),
    #[command(long_about = "Apply kubernetes manifest files")]
    Apply(ApplyArgs),
    #[command(long_about = "Refresh cluster state")]
    Refresh(RefreshArgs),
    #[command(long_about = "List resources")]
    Get(GetArgs),
    #[command(long_about = "View a resource")]
    Describe(DescribeArgs),
    #[command(long_about = "View resource logs")]
    Logs(LogArgs),
    #[command(long_about = "Configuration actions")]
    Config(ConfigArgs),
    #[command(long_about = "Taint a node as unschedulable")]
    Cordon(CordonArgs),
    #[command(long_about = "Remove unschedulable taint on a node")]
    Uncordon(UncordonArgs),
    #[command(long_about = "Cluster actions")]
    Cluster(ClusterArgs),
    #[command(long_about = "Rollout actions")]
    Rollout(RolloutArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigFileArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    pub skateconfig: String,
    #[arg(long, long_help = "Name of the context to use.")]
    pub context: Option<String>,
}

impl ApplyDeps for Deps {}
impl ClusterDeps for Deps {}
impl CreateDeps for Deps {}
impl DeleteDeps for Deps {}
impl CordonDeps for Deps {}
impl RefreshDeps for Deps{}
impl GetDeps for Deps{}
impl DescribeDeps for Deps{}
impl LogsDeps for Deps{}
impl RolloutDeps for Deps{}


pub async fn skate() -> Result<(), SkateError> {
    let deps = Deps {};

    config::ensure_config();
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => {
            let create = Create { deps };
            create.create(args).await
        }
        Commands::Delete(args) => {
            let delete = Delete { deps };
            delete.delete(args).await
        }

        Commands::Apply(args) => {
            let apply = Apply { deps, };
            apply.apply_self(args).await
        }
        Commands::Refresh(args) => {
            let refresh = Refresh{deps};
            refresh.refresh(args).await
        },
        Commands::Get(args) => {
            let get= Get{deps};
            get.get(args).await
        },
        Commands::Describe(args) => {
            let describe = Describe{deps};
            describe.describe(args).await
        },
        Commands::Logs(args) => {
            let logs= Logs{deps};
            logs.logs(args).await
        },
        Commands::Config(args) => crate::config_cmd::config(args),
        Commands::Cordon(args) => {
            let cordon = Cordon {deps};
            cordon.cordon(args).await
        }
        Commands::Uncordon(args) => {
            let cordon = Cordon {deps };
            cordon.uncordon(args).await
        }
        Commands::Cluster(args) => {
            let cluster = Cluster { deps };
            cluster.cluster(args).await
        }
        Commands::Rollout(args) => {
            let rollout = Rollout{deps};
            rollout.rollout(args).await
        },
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



