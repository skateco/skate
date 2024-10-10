use crate::controllers::pod::PodController;
use crate::skate::exec_cmd;
use k8s_openapi::api::apps::v1::Deployment;
use std::error::Error;
use crate::filestore::FileStore;
use crate::util::metadata_name;

pub struct DeploymentController {
    store: FileStore
}

impl DeploymentController {
    pub fn new(store: FileStore) -> Self {
        DeploymentController {
            store,
        }
    }
    
    pub fn apply(&self, deployment: Deployment) -> Result<(), Box<dyn Error>> {
        // store the deployment manifest on the node basically
        self.store.write_file("deployment", &metadata_name(&deployment).to_string(), "manifest.yaml", serde_yaml::to_string(&deployment)?.as_bytes())?;
        Ok(())
    }

    pub fn delete(&self, deployment: Deployment, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.clone().unwrap();
        let ns = deployment.metadata.namespace.clone().unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/deployment={}", name), "-q"])?;

        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        let pod_ctrl = PodController::new();
        pod_ctrl.delete_podman_pods(ids, grace_period)?;
        
        let _ = self.store.remove_object("deployment", &metadata_name(&deployment).to_string())?;
        Ok(())
    }
}