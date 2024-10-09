use crate::controllers::clusterissuer::ClusterIssuerController;
use crate::controllers::cronjob::CronjobController;
use crate::controllers::daemonset::DaemonSetController;
use crate::controllers::deployment::DeploymentController;
use crate::controllers::ingress::IngressController;
use crate::controllers::pod::PodController;
use crate::controllers::secret::SecretController;
use crate::controllers::service::ServiceController;
use crate::filestore::FileStore;
use crate::skate::SupportedResources;
use anyhow::anyhow;
use crate::errors::SkateError;

pub trait Executor {
    fn apply(&self, manifest: &str) -> Result<(), SkateError>;
    fn manifest_delete(&self, object: SupportedResources, grace: Option<usize>) -> Result<(), SkateError>;
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
    fn apply(&self, manifest: &str) -> Result<(),SkateError> {
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        match object {
            SupportedResources::Pod(pod) => {
                let ctrl = PodController::new();
                ctrl.apply(pod)?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new();
                ctrl.apply(secret)?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(self.store.clone());
                ctrl.apply(ingress)?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(self.store.clone());
                ctrl.apply(cron)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(self.store.clone());
                ctrl.apply(service)?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ctrl = ClusterIssuerController::new(self.store.clone());
                ctrl.apply(issuer)?;
            }
            _ => {
                return Err(anyhow!("unsupported resource type").into())
            }
        }
        Ok(())
    }


    fn manifest_delete(&self, object: SupportedResources, grace_period: Option<usize>) -> Result<(), SkateError> {
        match object {
            SupportedResources::Pod(p) => {
                let ctrl = PodController::new();
                ctrl.delete(p, grace_period)?;
            }
            SupportedResources::Deployment(d) => {
                let ctrl = DeploymentController::new();
                ctrl.delete(d, grace_period)?;
            }
            SupportedResources::DaemonSet(d) => {
                let ctrl = DaemonSetController::new();
                ctrl.delete(d, grace_period)?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(self.store.clone());
                ctrl.delete(ingress)?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(self.store.clone());
                ctrl.delete(cron)?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new();
                ctrl.delete(secret)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(self.store.clone());
                ctrl.delete(service)?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ctrl = ClusterIssuerController::new(self.store.clone());
                ctrl.delete(issuer)?;
            }
        }
        Ok(())
    }
}
