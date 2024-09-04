use crate::controllers::ingress::IngressController;
use crate::controllers::service::ServiceController;
use crate::controllers::cronjob::CronjobController;
use crate::filestore::FileStore;
use crate::skate::{exec_cmd, SupportedResources};
use crate::spec::cert::ClusterIssuer;
use crate::util::{apply_play, hash_string, metadata_name};
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::Secret;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, Write};
use std::str::FromStr;
use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;

pub trait Executor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>>;
    fn manifest_delete(&self, object: SupportedResources, grace: Option<usize>) -> Result<(), Box<dyn Error>>;
}

pub struct DefaultExecutor {
    store: FileStore,
}


impl DefaultExecutor {
    pub fn new() -> Self {
        DefaultExecutor {
            store: FileStore::new(),
        }
    }

}

impl Executor for DefaultExecutor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>> {
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        match object {
            SupportedResources::Pod(pod) => {
                let ctrl = PodController::new(self.store.clone());
                ctrl.apply(pod)
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new(self.store.clone());
                ctrl.apply(secret)
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(self.store.clone());
                ctrl.apply(ingress)
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(self.store.clone());
                ctrl.apply(cron)
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(self.store.clone());
                ctrl.apply(service)
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ctrl = ClusterIssuerController::new(self.store.clone());
                ctrl.apply(issuer)
            }
            _ => Err(anyhow!("unsupported resource type").into())
        }
    }


    fn manifest_delete(&self, object: SupportedResources, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        match object {
            SupportedResources::Pod(p) => {
                let ctrl = PodController::new(self.store.clone());
                ctrl.delete(p, grace_period)
            }
            SupportedResources::Deployment(d) => {
                let ctrl = DeploymentController::new(self.store.clone());
                ctrl.delete(d, grace_period)
            }
            SupportedResources::DaemonSet(d) => {
                let ctrl = DaemonSetController::new(self.store.clone());
                ctrl.delete(d, grace_period)
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(self.store.clone());
                ctrl.delete(ingress)
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(self.store.clone());
                ctrl.delete(cron)
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new(self.store.clone());
                ctrl.delete(secret)
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(self.store.clone());
                ctrl.delete(service)
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ctrl = ClusterIssuerController::new(self.store.clone());
                ctrl.delete(issuer)
            }
        }
    }
}
