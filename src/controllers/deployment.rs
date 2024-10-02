use crate::controllers::pod::PodController;
use crate::skate::exec_cmd;
use k8s_openapi::api::apps::v1::Deployment;
use std::error::Error;

pub struct DeploymentController {
}

impl DeploymentController {
    pub fn new() -> Self {
        DeploymentController {
        }
    }

    pub fn delete(&self, deployment: Deployment, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.unwrap();
        let ns = deployment.metadata.namespace.unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/deployment={}", name), "-q"])?;

        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        let pod_ctrl = PodController::new();
        pod_ctrl.delete_podman_pods(ids, grace_period)
    }
}