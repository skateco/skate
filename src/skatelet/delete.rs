use crate::skatelet::apply::StdinCommand;
use crate::supported_resources::SupportedResources;
use crate::supported_resources::SupportedResources::{ClusterIssuer, CronJob, Ingress, Service};
use clap::{Args, Subcommand};
use std::collections::BTreeMap;
use std::io;
use std::io::Read;

use k8s_openapi::api::batch::v1::CronJob as K8sCronJob;
use k8s_openapi::api::core::v1::Secret;

use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::cronjob::CronjobController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::ingress::IngressController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;
use crate::controllers::service::ServiceController;
use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::skatelet::VAR_PATH;
use crate::spec;
use crate::util::SkateLabels;
use k8s_openapi::api::core::v1::Service as K8sService;
use k8s_openapi::api::networking::v1::Ingress as K8sIngress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

#[derive(Debug, Args, Clone)]
pub struct DeleteResourceArgs {
    #[arg(long, long_help = "Name of the resource.")]
    pub name: String,
    #[arg(long, long_help = "Name of the resource.")]
    pub namespace: String,
}

#[derive(Debug, Subcommand, Clone)]
pub enum DeleteResourceCommands {
    #[command(flatten)]
    StdinCommand(StdinCommand),
    Ingress(DeleteResourceArgs),
    Cronjob(DeleteResourceArgs),
    Secret(DeleteResourceArgs),
    Deployment(DeleteResourceArgs),
    Daemonset(DeleteResourceArgs),
    Service(DeleteResourceArgs),
    Clusterissuer(DeleteResourceArgs),
}

#[derive(Debug, Args, Clone)]
pub struct DeleteArgs {
    #[arg(
        short,
        long,
        long_help("Number of seconds to wait before hard killing.")
    )]
    termination_grace_period: Option<usize>,
    #[command(subcommand)]
    command: DeleteResourceCommands,
}

pub trait DeleteDeps: With<dyn Store> + With<dyn ShellExec> + WithDB {}

pub struct Deleter<D: DeleteDeps> {
    pub deps: D,
}

impl<D: DeleteDeps> Deleter<D> {
    fn store(&self) -> Box<dyn Store> {
        With::<dyn Store>::get(&self.deps)
    }
    fn execer(&self) -> Box<dyn ShellExec> {
        With::<dyn ShellExec>::get(&self.deps)
    }
    pub async fn delete(&self, args: DeleteArgs) -> Result<(), SkateError> {
        match &args.command {
            DeleteResourceCommands::Ingress(resource_args) => {
                self.delete_ingress(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::StdinCommand(_) => self.delete_stdin(args).await,
            DeleteResourceCommands::Cronjob(resource_args) => {
                self.delete_cronjob(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::Secret(resource_args) => {
                self.delete_secret(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::Daemonset(resource_args) => {
                self.delete_daemonset(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::Deployment(resource_args) => {
                self.delete_deployment(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::Service(resource_args) => {
                self.delete_service(args.clone(), resource_args.clone())
                    .await
            }
            DeleteResourceCommands::Clusterissuer(resource_args) => {
                self.delete_cluster_issuer(args.clone(), resource_args.clone())
                    .await
            }
        }
    }

    fn deletion_metadata(resource_args: DeleteResourceArgs) -> ObjectMeta {
        let mut meta = ObjectMeta::default();
        meta.name = Some(resource_args.name.clone());
        meta.namespace = Some(resource_args.namespace.clone());
        meta.labels = Some(BTreeMap::from([
            (SkateLabels::Name.to_string(), resource_args.name),
            (SkateLabels::Namespace.to_string(), resource_args.namespace),
        ]));
        meta
    }

    async fn delete_ingress(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &Ingress(K8sIngress {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
                status: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_service(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &Service(K8sService {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
                status: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_cluster_issuer(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &ClusterIssuer(spec::cert::ClusterIssuer {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_cronjob(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &CronJob(K8sCronJob {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
                status: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_secret(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &SupportedResources::Secret(Secret {
                data: None,
                immutable: None,
                metadata: Self::deletion_metadata(resource_args),
                string_data: None,
                type_: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_stdin(&self, args: DeleteArgs) -> Result<(), SkateError> {
        let manifest = {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        };

        let object: SupportedResources =
            serde_yaml::from_str(&manifest).expect("failed to deserialize manifest");
        self.manifest_delete(&object, args.termination_grace_period)
            .await
    }

    async fn delete_deployment(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &SupportedResources::Deployment(k8s_openapi::api::apps::v1::Deployment {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
                status: None,
            }),
            delete_args.termination_grace_period,
        )
        .await
    }

    async fn delete_daemonset(
        &self,
        delete_args: DeleteArgs,
        resource_args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        self.manifest_delete(
            &SupportedResources::DaemonSet(k8s_openapi::api::apps::v1::DaemonSet {
                metadata: Self::deletion_metadata(resource_args),
                spec: None,
                status: None,
            }),
            delete_args.termination_grace_period,
        )
        .await?;
        Ok(())
    }

    async fn manifest_delete(
        &self,
        object: &SupportedResources,
        grace_period: Option<usize>,
    ) -> Result<(), SkateError> {
        match object {
            SupportedResources::Pod(p) => {
                let ctrl = PodController::new(With::<dyn ShellExec>::get(&self.deps));
                ctrl.delete(p, grace_period)?;
            }
            SupportedResources::Deployment(d) => {
                let pod_controller = PodController::new(self.execer());
                let ctrl =
                    DeploymentController::new(self.deps.get_db(), self.execer(), pod_controller);
                ctrl.delete(d, grace_period).await?;
            }
            SupportedResources::DaemonSet(d) => {
                let pod_controller = PodController::new(self.execer());
                let ctrl =
                    DaemonSetController::new(self.deps.get_db(), self.execer(), pod_controller);
                ctrl.delete(d, grace_period).await?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(self.deps.get_db(), self.execer());
                ctrl.delete(ingress).await?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(self.store(), self.deps.get_db(), self.execer());
                ctrl.delete(cron).await?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new(self.execer());
                ctrl.delete(secret)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(
                    self.deps.get_db(),
                    self.execer(),
                    VAR_PATH,
                    "/etc/systemd/system",
                );
                ctrl.delete(service).await?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ingress_controller = IngressController::new(self.deps.get_db(), self.execer());
                let ctrl = ClusterIssuerController::new(self.deps.get_db(), ingress_controller);
                ctrl.delete(issuer).await?;
            }
        }
        Ok(())
    }
}
