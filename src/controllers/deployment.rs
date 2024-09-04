use std::error::Error;
use k8s_openapi::api::apps::v1::Deployment;
use crate::controllers::pod::PodController;
use crate::filestore::FileStore;
use crate::skate::exec_cmd;

pub struct DeploymentController {
    store: FileStore,
}

impl DeploymentController {
    pub fn new(file_store: FileStore) -> Self {
        DeploymentController {
            store: file_store,
        }
    }

    pub fn delete(&self, deployment: Deployment, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.unwrap();
        let ns = deployment.metadata.namespace.unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/deployment={}", name), "-q"])?;

        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        let pod_ctrl = PodController::new(self.store.clone());
        pod_ctrl.delete_podman_pods(ids, grace_period)
    }
}