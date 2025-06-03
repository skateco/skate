use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::refresh::{Refresh, RefreshDeps};
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skate::ConfigFileArgs;
use crate::ssh::SshClients;
use crate::state::state::ClusterState;
use crate::supported_resources::SupportedResources;
use clap::{Args, Subcommand};
use std::error::Error;

#[derive(Debug, Args)]
pub struct ClusterArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(
        long_about = "Re-apply all resources in the cluster. Useful after cordon/uncordon or node creation"
    )]
    Reschedule(RescheduleArgs),
}

#[derive(Debug, Args)]
pub struct RescheduleArgs {
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[arg(long, long_help = "Will not affect the cluster if set to true")]
    pub dry_run: bool,
}

pub trait ClusterDeps: With<dyn SshManager> + RefreshDeps {}

pub struct Cluster<D: ClusterDeps> {
    pub deps: D,
}

impl<D: ClusterDeps> Cluster<D> {
    pub async fn cluster(&self, global_args: ClusterArgs) -> Result<(), SkateError> {
        match global_args.command {
            Commands::Reschedule(args) => {
                let mut args = args;
                args.config = global_args.config;
                self.reschedule(args).await
            }
        }
    }

    pub async fn reschedule(&self, args: RescheduleArgs) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;

        let cluster = config.active_cluster(config.current_context.clone())?;

        let ssh_mgr = self.deps.get();

        let (conns, _) = ssh_mgr.cluster_connect(cluster).await;

        let conns = conns.ok_or("failed to get cluster connections".to_string())?;

        let state = Refresh::<D>::refreshed_state(&cluster.name, &conns, &config).await?;

        self.propagate_existing_resources(&conns, None, &state, args.dry_run)
            .await?;

        Ok(())
    }

    async fn propagate_existing_resources(
        &self,
        all_conns: &SshClients,
        exclude_donor_node: Option<&str>,
        state: &ClusterState,
        dry_run: bool,
    ) -> Result<(), Box<dyn Error>> {
        let catalogue = state.catalogue(None, &[], None, None);

        let all_manifests: Result<Vec<SupportedResources>, _> = catalogue
            .iter()
            .map(|item| SupportedResources::try_from(item.object))
            .collect();
        let all_manifests = all_manifests?;

        let mut filtered_state = state.clone();
        filtered_state
            .nodes
            .retain(|n| exclude_donor_node.is_none() || n.node_name != exclude_donor_node.unwrap());

        println!(
            "rescheduling {} resources across {} nodes",
            all_manifests.len(),
            filtered_state.nodes.len()
        );

        let scheduler = DefaultScheduler::new();

        scheduler
            .schedule(all_conns, &mut filtered_state, all_manifests, dry_run)
            .await?;

        Ok(())
    }
}
