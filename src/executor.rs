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
use crate::exec::RealExec;

pub struct DefaultExecutor {
}


impl DefaultExecutor {
    pub fn new() -> Self {
        DefaultExecutor {
        }
    }
}

impl DefaultExecutor {
    pub fn apply(&self, object: &SupportedResources) -> Result<(),SkateError> {

        let store = Box::new(FileStore::new());
        let execer = Box::new(RealExec{});
        
        match object {
            SupportedResources::Deployment(deployment) => {
                let pod_controller = PodController::new(execer.clone());
                let ctrl = DeploymentController::new(store, execer,pod_controller);
                ctrl.apply(deployment)?;
            }
            SupportedResources::DaemonSet(daemonset) => {
                let pod_controller = PodController::new(execer.clone());
                let ctrl = DaemonSetController::new(store, execer, pod_controller);
                ctrl.apply(daemonset)?;
            }
            SupportedResources::Pod(pod) => {
                let ctrl = PodController::new(execer);
                ctrl.apply(pod)?;
            }
            SupportedResources::Secret(secret) => {
                let ctrl = SecretController::new(execer);
                ctrl.apply(secret)?;
            }
            SupportedResources::Ingress(ingress) => {
                let ctrl = IngressController::new(store, execer);
                ctrl.apply(ingress)?;
            }
            SupportedResources::CronJob(cron) => {
                let ctrl = CronjobController::new(store, execer);
                ctrl.apply(cron)?;
            }
            SupportedResources::Service(service) => {
                let ctrl = ServiceController::new(store, execer);
                ctrl.apply(service)?;
            }
            SupportedResources::ClusterIssuer(issuer) => {
                let ingress_ctrl = IngressController::new(store.clone(), execer.clone());
                let ctrl = ClusterIssuerController::new(store, ingress_ctrl);
                ctrl.apply(issuer)?;
            }
        }
        Ok(())
    }


    pub fn manifest_delete(&self, object: &SupportedResources, grace_period: Option<usize>) -> Result<(), SkateError> {

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
}
