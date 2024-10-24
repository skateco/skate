use std::collections::BTreeMap;
use std::io;
use std::io::Read;
use clap::{Args, Subcommand};
use crate::resource::SupportedResources;
use crate::resource::SupportedResources::{ClusterIssuer, CronJob, Ingress, Service};
use crate::skatelet::apply::StdinCommand;

use k8s_openapi::api::batch::v1::CronJob as K8sCronJob;
use k8s_openapi::api::core::v1::Secret;

use k8s_openapi::api::networking::v1::Ingress as K8sIngress;
use k8s_openapi::api::core::v1::Service as K8sService;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::cronjob::CronjobController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::ingress::IngressController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;
use crate::controllers::service::ServiceController;
use crate::errors::SkateError;
use crate::exec::RealExec;
use crate::filestore::FileStore;
use crate::spec;

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
    #[arg(short, long, long_help("Number of seconds to wait before hard killing."))]
    termination_grace_period: Option<usize>,
    #[command(subcommand)]
    command: DeleteResourceCommands,
}

pub fn delete(args: DeleteArgs) -> Result<(), SkateError> {
    match &args.command {
        DeleteResourceCommands::Ingress(resource_args) => delete_ingress(args.clone(), resource_args.clone()),
        DeleteResourceCommands::StdinCommand(_) => delete_stdin(args),
        DeleteResourceCommands::Cronjob(resource_args) => delete_cronjob(args.clone(), resource_args.clone()),
        DeleteResourceCommands::Secret(resource_args) => delete_secret(args.clone(), resource_args.clone()),
        DeleteResourceCommands::Daemonset(resource_args) => delete_daemonset(args.clone(), resource_args.clone()),
        DeleteResourceCommands::Deployment(resource_args) => delete_deployment(args.clone(), resource_args.clone()),
        DeleteResourceCommands::Service(resource_args) => delete_service(args.clone(), resource_args.clone()),
        DeleteResourceCommands::Clusterissuer(resource_args) => delete_cluster_issuer(args.clone(), resource_args.clone())
    }
}


fn deletion_metadata(resource_args: DeleteResourceArgs) -> ObjectMeta {
    let mut meta = ObjectMeta::default();
    meta.name = Some(resource_args.name.clone());
    meta.namespace = Some(resource_args.namespace.clone());
    meta.labels = Some(BTreeMap::from([
        ("skate.io/name".to_string(), resource_args.name),
        ("skate.io/namespace".to_string(), resource_args.namespace),
    ]));
    meta
}

pub fn delete_ingress(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

    manifest_delete(&Ingress(K8sIngress {
        metadata: deletion_metadata(resource_args),
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_service(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

    manifest_delete(&Service(K8sService {
        metadata: deletion_metadata(resource_args),
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_cluster_issuer(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

    manifest_delete(&ClusterIssuer(spec::cert::ClusterIssuer {
        metadata: deletion_metadata(resource_args),
        spec: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_cronjob(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

    manifest_delete(&CronJob(K8sCronJob {
        metadata: deletion_metadata(resource_args),
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_secret(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {
    manifest_delete(&SupportedResources::Secret(Secret {
        data: None,
        immutable: None,
        metadata: deletion_metadata(resource_args),
        string_data: None,
        type_: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_stdin(args: DeleteArgs) -> Result<(), SkateError> {
    let manifest = {
        let mut stdin = io::stdin();
        let mut buffer = String::new();
        stdin.read_to_string(&mut buffer)?;
        buffer
    };

    let object: SupportedResources = serde_yaml::from_str(&manifest).expect("failed to deserialize manifest");
    manifest_delete(&object, args.termination_grace_period)
}

pub fn delete_deployment(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

   manifest_delete(&SupportedResources::Deployment(k8s_openapi::api::apps::v1::Deployment {
        metadata: deletion_metadata(resource_args),
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_daemonset(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), SkateError> {

    manifest_delete(&SupportedResources::DaemonSet(k8s_openapi::api::apps::v1::DaemonSet {
        metadata: deletion_metadata(resource_args),
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)?;
    Ok(())
}

fn manifest_delete(object: &SupportedResources, grace_period: Option<usize>) -> Result<(), SkateError> {

    let store = Box::new(FileStore::new());
    let execer = Box::new(RealExec{});

    match object {
        SupportedResources::Pod(p) => {
            let ctrl = PodController::new(execer);
            ctrl.delete(p, grace_period)?;
        }
        SupportedResources::Deployment(d) => {
            let pod_controller = PodController::new(execer.clone());
            let ctrl = DeploymentController::new(store, execer, pod_controller);
            ctrl.delete(d, grace_period)?;
        }
        SupportedResources::DaemonSet(d) => {
            let pod_controller = PodController::new(execer.clone());
            let ctrl = DaemonSetController::new(store, execer, pod_controller);
            ctrl.delete(d, grace_period)?;
        }
        SupportedResources::Ingress(ingress) => {
            let ctrl = IngressController::new(store, execer);
            ctrl.delete(ingress)?;
        }
        SupportedResources::CronJob(cron) => {
            let ctrl = CronjobController::new(store, execer);
            ctrl.delete(cron)?;
        }
        SupportedResources::Secret(secret) => {
            let ctrl = SecretController::new(execer);
            ctrl.delete(secret)?;
        }
        SupportedResources::Service(service) => {
            let ctrl = ServiceController::new(store, execer);
            ctrl.delete(service)?;
        }
        SupportedResources::ClusterIssuer(issuer) => {
            let ingress_controller = IngressController::new(store.clone(), execer.clone());
            let ctrl = ClusterIssuerController::new(store, ingress_controller);
            ctrl.delete(issuer)?;
        }
    }
    Ok(())
}
