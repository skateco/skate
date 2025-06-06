#![allow(unused)]

use crate::apply::{Apply, ApplyArgs, ApplyDeps};
use crate::cluster::{Cluster, ClusterArgs, ClusterDeps};
use crate::config;
use crate::config_cmd::ConfigArgs;
use crate::cordon::{Cordon, CordonArgs, CordonDeps, UncordonArgs};
use crate::create::{Create, CreateArgs, CreateDeps};
use crate::delete::{Delete, DeleteArgs, DeleteDeps};
use crate::deps::SkateDeps;
use crate::describe::{Describe, DescribeArgs, DescribeDeps};
use crate::errors::SkateError;
use crate::get::{Get, GetArgs, GetDeps};
use crate::logs::{LogArgs, Logs, LogsDeps};
use crate::node_shell::{NodeShell, NodeShellArgs, NodeShellDeps};
use crate::refresh::{Refresh, RefreshArgs, RefreshDeps};
use crate::rollout::{Rollout, RolloutArgs, RolloutDeps};
use crate::skate::Distribution::{Debian, Fedora, Raspbian, Ubuntu, Unknown};
use crate::upgrade::{Upgrade, UpgradeArgs, UpgradeDeps};
use crate::util;
use clap::{Args, Parser, Subcommand};
use env_logger::{Builder, Env};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, SqliteConnection};
use std::fmt::{Display, Formatter};
use strum_macros::Display;

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true, version)]
#[clap(version = util::version(false), long_version = util::version(true))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(
        long,
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        long_help = "Increase verbosity. Use multiple times, up to a max of -vvv for more verbosity. Levels are 'info', 'debug', and 'trace'. Default is 'off'.",
    )]
    verbose: u8,
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
    #[command(long_about = "Start a shell on a node")]
    NodeShell(NodeShellArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigFileArgs {
    #[arg(
        long,
        long_help = "Configuration for skate.",
        global = true,
        default_value = "~/.skate/config.yaml"
    )]
    pub skateconfig: String,
    #[arg(long, long_help = "Name of the context to use.", global = true)]
    pub context: Option<String>,
}

impl ApplyDeps for SkateDeps {}
impl ClusterDeps for SkateDeps {}
impl CreateDeps for SkateDeps {}
impl DeleteDeps for SkateDeps {}
impl CordonDeps for SkateDeps {}
impl RefreshDeps for SkateDeps {}
impl GetDeps for SkateDeps {}
impl DescribeDeps for SkateDeps {}
impl LogsDeps for SkateDeps {}
impl RolloutDeps for SkateDeps {}

impl UpgradeDeps for SkateDeps {}

impl NodeShellDeps for SkateDeps {}

pub trait AllDeps:
    ApplyDeps
    + ClusterDeps
    + CreateDeps
    + DeleteDeps
    + CordonDeps
    + RefreshDeps
    + GetDeps
    + DescribeDeps
    + LogsDeps
    + RolloutDeps
    + UpgradeDeps
    + NodeShellDeps
{
}

impl AllDeps for SkateDeps {}

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
            let apply = Apply { deps };
            apply.apply_self(args).await
        }
        Commands::Refresh(args) => {
            let refresh = Refresh { deps };
            refresh.refresh(args).await
        }
        Commands::Get(args) => {
            let get = Get { deps };
            get.get(args).await
        }
        Commands::Describe(args) => {
            let describe = Describe { deps };
            describe.describe(args).await
        }
        Commands::Logs(args) => {
            let logs = Logs { deps };
            logs.logs(args).await
        }
        Commands::Config(args) => crate::config_cmd::config(args),
        Commands::Cordon(args) => {
            let cordon = Cordon { deps };
            cordon.cordon(args).await
        }
        Commands::Uncordon(args) => {
            let cordon = Cordon { deps };
            cordon.uncordon(args).await
        }
        Commands::Cluster(args) => {
            let cluster = Cluster { deps };
            cluster.cluster(args).await
        }
        Commands::Rollout(args) => {
            let rollout = Rollout { deps };
            rollout.rollout(args).await
        }
        Commands::Upgrade(args) => {
            let upgrade = Upgrade { deps };
            upgrade.upgrade(args).await
        }
        Commands::NodeShell(args) => {
            let node_shell = NodeShell { deps };
            node_shell.node_shell(args).await
        }
    }?;
    Ok(())
}

pub async fn skate<D: AllDeps>(deps: D) -> Result<(), SkateError> {
    let args = Cli::parse();

    env_logger::builder()
        .filter_module("skate", count_to_log_level(args.verbose))
        .format_target(false)
        .format_timestamp(None)
        .init();
    skate_with_args(deps, args).await
}

fn count_to_log_level(count: u8) -> log::LevelFilter {
    match count {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Platform {
    pub arch: String,
    pub distribution: Distribution,
}

impl Platform {
    pub fn arch_as_linux_target_triple(&self) -> (&str, &str, &str) {
        match self.arch.as_str() {
            "amd64" => ("x86_64", "unknown-linux", "gnu"),
            "armv6l" => ("arm", "unknown-linux", "gnueabi"),
            "armv7l" => ("arm7", "unknown-linux", "gnueabi"),
            "arm64" => ("aarch64", "unknown-linux", "gnu"),
            _ => (self.arch.as_str(), "unknown-linux", "gnu"),
        }
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "arch: {}, distribution: {}",
            self.arch, self.distribution
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Display)]
pub enum Distribution {
    #[default]
    Unknown,
    Debian,
    Raspbian,
    Ubuntu,
    Fedora,
}

impl From<&str> for Distribution {
    fn from(s: &str) -> Self {
        match s.to_lowercase().trim_matches(|c| c == '\'' || c == '"') {
            s if s.starts_with("debian") => Debian,
            s if s.starts_with("raspbian") => Raspbian,
            s if s.starts_with("ubuntu") => Ubuntu,
            s if s.starts_with("fedora") => Fedora,
            _ => Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::apply::ApplyDeps;
    use crate::cluster::ClusterDeps;
    use crate::cordon::CordonDeps;
    use crate::create::CreateDeps;
    use crate::delete::DeleteDeps;
    use crate::deps::{SshManager, With};
    use crate::describe::DescribeDeps;
    use crate::get::GetDeps;
    use crate::logs::LogsDeps;
    use crate::node_shell::NodeShellDeps;
    use crate::refresh::{RefreshArgs, RefreshDeps};
    use crate::rollout::RolloutDeps;
    use crate::skate::Commands::Refresh;
    use crate::skate::Distribution::{Debian, Fedora, Raspbian, Ubuntu, Unknown};
    use crate::skate::{skate_with_args, Cli, ConfigFileArgs, Distribution, Platform};
    use crate::test_helpers::ssh_mocks::MockSshManager;
    use crate::upgrade::UpgradeDeps;
    use crate::{skate, AllDeps};

    struct TestDeps {}

    impl With<dyn SshManager> for TestDeps {
        fn get(&self) -> Box<dyn SshManager> {
            Box::new(MockSshManager {}) as Box<dyn SshManager>
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
    impl NodeShellDeps for TestDeps {}

    impl AllDeps for TestDeps {}

    #[tokio::test]
    async fn test_runs() {
        let deps = TestDeps {};

        skate_with_args(
            deps,
            Cli {
                verbose: 0,
                command: Refresh(RefreshArgs {
                    json: false,
                    config: ConfigFileArgs {
                        skateconfig: "".to_string(),
                        context: None,
                    },
                }),
            },
        )
        .await;
    }

    #[test]
    fn test_distribution_from_str() {
        assert_eq!(Distribution::from("Debian"), Debian);
        assert_eq!(Distribution::from("Raspbian"), Raspbian);
        assert_eq!(Distribution::from("Ubuntu"), Ubuntu);
        assert_eq!(Distribution::from(r#""Fedora Linux""#), Fedora);
        assert_eq!(Distribution::from("unknown"), Unknown);
    }
}
