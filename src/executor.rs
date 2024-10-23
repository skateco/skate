use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::cronjob::CronjobController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::ingress::IngressController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;
use crate::controllers::service::ServiceController;
use crate::filestore::{FileStore, Store};
use crate::resource::SupportedResources;
use crate::errors::SkateError;


pub struct DefaultExecutor {
}


impl DefaultExecutor {
    pub fn new() -> Self {
        DefaultExecutor {
        }
    }
}

impl DefaultExecutor {
    pub fn apply(&self, manifest: &str) -> Result<(),SkateError> {
        
        let store = Box::new(FileStore::new());
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        match object {
            SupportedResources::Deployment(deployment) => {
                let ctrl = DeploymentController::new(store);
                ctrl.apply(deployment)?;
            }
            SupportedResources::DaemonSet(daemonset) => {
                let ctrl = DaemonSetController::new(store);
                ctrl.apply(daemonset)?;
            }
            SupportedResources::Pod(pod) => {
                let ctrl = PodController::new();
                ctrl.apply(pod)?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new();
                ctrl.apply(secret)?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(store);
                ctrl.apply(ingress)?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(store);
                ctrl.apply(cron)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(store);
                ctrl.apply(service)?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ingress_ctrl = IngressController::new(store.clone());
                let ctrl = ClusterIssuerController::new(store, ingress_ctrl);
                ctrl.apply(issuer)?;
            }
        }
        Ok(())
    }


    pub fn manifest_delete(&self, object: SupportedResources, grace_period: Option<usize>) -> Result<(), SkateError> {

        let store = Box::new(FileStore::new());
        
        match object {
            SupportedResources::Pod(p) => {
                let ctrl = PodController::new();
                ctrl.delete(p, grace_period)?;
            }
            SupportedResources::Deployment(d) => {
                let ctrl = DeploymentController::new(store);
                ctrl.delete(d, grace_period)?;
            }
            SupportedResources::DaemonSet(d) => {
                let ctrl = DaemonSetController::new(store);
                ctrl.delete(d, grace_period)?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(store);
                ctrl.delete(ingress)?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(store);
                ctrl.delete(cron)?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new();
                ctrl.delete(secret)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(store);
                ctrl.delete(service)?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ingress_controller = IngressController::new(store.clone());
                let ctrl = ClusterIssuerController::new(store, ingress_controller);
                ctrl.delete(issuer)?;
            }
        }
        Ok(())
    }
}
