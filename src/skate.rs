#![allow(unused)]

use crate::util;
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use crate::apply::{Apply, ApplyArgs, ApplyDeps};
use crate::refresh::{Refresh, RefreshArgs, RefreshDeps};
use strum_macros::Display;
use std::fmt::{Display, Formatter};
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
use crate::rollout::{Rollout, RolloutArgs, RolloutDeps};
use crate::skate::Distribution::{Debian, Raspbian, Ubuntu, Unknown};
use crate::upgrade::{Upgrade, UpgradeArgs, UpgradeDeps};




#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true, version)]
#[clap(version = util::version(false), long_version = util::version(true))]
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
    #[command(long_about = "Upgrade actions")]
    Upgrade(UpgradeArgs),
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

impl UpgradeDeps for Deps{}

pub trait AllDeps: ApplyDeps + ClusterDeps + CreateDeps + DeleteDeps + CordonDeps + RefreshDeps + GetDeps + DescribeDeps + LogsDeps + RolloutDeps + UpgradeDeps{} 

impl AllDeps for Deps{}

async fn skate_with_args<D: AllDeps>(deps: D, args: Cli) -> Result<(), SkateError> {
    config::ensure_config();
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
        Commands::Upgrade(args) => {
            let upgrade = Upgrade{deps};
            upgrade.upgrade(args).await
        }
    }?;
    Ok(())
}

pub async fn skate<D: AllDeps>(deps: D) -> Result<(), SkateError> {
    let args = Cli::parse();
    skate_with_args(deps, args).await
}


#[derive(Debug, Clone, PartialEq,Default, Serialize, Deserialize)]
pub struct Platform {
    pub arch: String,
    pub distribution: Distribution,
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("arch: {}, distribution: {}", self.arch, self.distribution))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Display)]
pub enum Distribution {
    #[default]
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

#[cfg(test)]
mod tests {
    use crate::{skate, AllDeps};
    use crate::apply::ApplyDeps;
    use crate::cluster::ClusterDeps;
    use crate::cordon::CordonDeps;
    use crate::create::CreateDeps;
    use crate::delete::DeleteDeps;
    use crate::deps::{SshManager, With};
    use crate::describe::DescribeDeps;
    use crate::get::GetDeps;
    use crate::logs::LogsDeps;
    use crate::refresh::{RefreshArgs, RefreshDeps};
    use crate::rollout::RolloutDeps;
    use crate::skate::{skate_with_args, Cli, ConfigFileArgs};
    use crate::skate::Commands::Refresh;
    use crate::test_helpers::ssh_mocks::MockSshManager;
    use crate::upgrade::UpgradeDeps;

    struct TestDeps{}

    impl With<dyn SshManager> for TestDeps{
        fn get(&self) -> Box<dyn SshManager> {
            Box::new(MockSshManager{}) as Box<dyn SshManager>
        }
    }

    impl ApplyDeps for TestDeps {}
    impl RefreshDeps for TestDeps {}
    impl ClusterDeps for TestDeps {}
    impl CreateDeps for TestDeps {}
    impl DeleteDeps for TestDeps {}
    impl CordonDeps for TestDeps {}
    impl GetDeps for TestDeps {}
    impl DescribeDeps for TestDeps {}
    impl LogsDeps for TestDeps {}
    impl RolloutDeps for TestDeps {}
    impl UpgradeDeps for TestDeps {}

    impl AllDeps for TestDeps{}

    #[tokio::test]
    async fn test_runs() {
        let deps = TestDeps{};

        skate_with_args(deps, Cli{ command: Refresh(RefreshArgs{
            json: false,
            config: ConfigFileArgs{
                skateconfig: "".to_string(),
                context: None,
            },
        }) }).await;

    }
}





