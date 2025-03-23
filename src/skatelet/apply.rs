use clap::{Args, Subcommand};

use std::io;

use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::cronjob::CronjobController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::ingress::IngressController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;
use crate::controllers::service::ServiceController;
use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::resource::SupportedResources;
use crate::skatelet::VAR_PATH;
use std::io::Read;

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(
        short,
        long,
        long_help("Delete previously applied objects that are not in the set passed to the current invocation."
        )
    )]
    prune: bool,
    #[command(subcommand)]
    command: StdinCommand,
}

#[derive(Debug, Subcommand, Clone)]
pub enum StdinCommand {
    #[command(name = "-", about = "feed manifest yaml via stdin")]
    Stdin {},
}

pub trait ApplyDeps: With<dyn Store> + With<dyn ShellExec> {}

pub fn apply<D: ApplyDeps>(deps: D, apply_args: ApplyArgs) -> Result<(), SkateError> {
    let manifest = match apply_args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };

    let object: SupportedResources =
        serde_yaml::from_str(&manifest).expect("failed to deserialize manifest");
    apply_supported_resource(deps, &object)
}

fn apply_supported_resource<D: ApplyDeps>(
    deps: D,
    object: &SupportedResources,
) -> Result<(), SkateError> {
    let execer = With::<dyn ShellExec>::get;
    let store = With::<dyn Store>::get;

    match object {
        SupportedResources::Deployment(deployment) => {
            let pod_controller = PodController::new(execer(&deps));
            let ctrl = DeploymentController::new(store(&deps), execer(&deps), pod_controller);
            ctrl.apply(deployment)?;
        }
        SupportedResources::DaemonSet(daemonset) => {
            let pod_controller = PodController::new(execer(&deps));
            let ctrl = DaemonSetController::new(store(&deps), execer(&deps), pod_controller);
            ctrl.apply(daemonset)?;
        }
        SupportedResources::Pod(pod) => {
            let ctrl = PodController::new(execer(&deps));
            ctrl.apply(pod)?;
        }
        SupportedResources::Secret(secret) => {
            let ctrl = SecretController::new(execer(&deps));
            ctrl.apply(secret)?;
        }
        SupportedResources::Ingress(ingress) => {
            let ctrl = IngressController::new(store(&deps), execer(&deps));
            ctrl.apply(ingress)?;
        }
        SupportedResources::CronJob(cron) => {
            let ctrl = CronjobController::new(store(&deps), execer(&deps));
            ctrl.apply(cron)?;
        }
        SupportedResources::Service(service) => {
            let ctrl = ServiceController::new(
                store(&deps),
                execer(&deps),
                VAR_PATH,
                "/etc/systemd/system",
            );
            ctrl.apply(service)?;
        }
        SupportedResources::ClusterIssuer(issuer) => {
            let ingress_ctrl = IngressController::new(store(&deps), execer(&deps));
            let ctrl = ClusterIssuerController::new(store(&deps), ingress_ctrl);
            ctrl.apply(issuer)?;
        }
    }
    Ok(())
}
