use crate::controllers::pod::PodController;
use crate::exec::{ShellExec};
use crate::filestore::Store;
use crate::util::metadata_name;
use k8s_openapi::api::apps::v1::Deployment;
use std::error::Error;

pub struct DeploymentController {
    store: Box<dyn Store>,
    execer: Box<dyn ShellExec>,
    pod_controller: PodController
}

impl DeploymentController {
    pub fn new(store: Box<dyn Store>, execer: Box<dyn ShellExec>, pod_controller: PodController) -> Self {
        DeploymentController {
            store,
            execer,
            pod_controller
        }
    }

    pub fn apply(&self, deployment: &Deployment) -> Result<(), Box<dyn Error>> {
        // store the deployment manifest on the node basically
        self.store.write_file("deployment", &metadata_name(deployment).to_string(), "manifest.yaml", serde_yaml::to_string(&deployment)?.as_bytes())?;

        let ns_name = metadata_name(deployment);
        let hash = deployment.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();
        if !hash.is_empty() {
            self.store.write_file("deployment", &ns_name.to_string(), "hash", hash.as_bytes())?;
        }
        Ok(())
    }

    pub fn delete(&self, deployment: &Deployment, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.clone().unwrap();
        let ns = deployment.metadata.namespace.clone().unwrap_or("default".to_string());

        let ids = self.execer.exec("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/deployment={}", name), "-q"])?;

        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        self.pod_controller.delete_podman_pods(ids, grace_period)?;
        
        let _ = self.store.remove_object("deployment", &metadata_name(deployment).to_string())?;
        Ok(())
    }
}