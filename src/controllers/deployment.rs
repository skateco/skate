use crate::controllers::pod::PodController;
use crate::exec::ShellExec;
use crate::skatelet::database::resource;
use crate::skatelet::database::resource::{delete_resource, insert_resource, ResourceType};
use crate::util::metadata_name;
use k8s_openapi::api::apps::v1::Deployment;
use sqlx::SqlitePool;
use std::error::Error;

pub struct DeploymentController {
    db: SqlitePool,
    execer: Box<dyn ShellExec>,
    pod_controller: PodController,
}

impl DeploymentController {
    pub fn new(db: SqlitePool, execer: Box<dyn ShellExec>, pod_controller: PodController) -> Self {
        Self {
            db,
            execer,
            pod_controller,
        }
    }

    pub async fn apply(&self, deployment: &Deployment) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(deployment);

        let hash = deployment
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        let object = resource::Resource {
            name: ns_name.name.clone(),
            namespace: ns_name.namespace.clone(),
            resource_type: resource::ResourceType::Deployment,
            manifest: serde_json::to_value(deployment)?,
            hash: hash.clone(),
            ..Default::default()
        };
        insert_resource(&self.db, &object).await?;

        Ok(())
    }

    pub async fn delete(
        &self,
        deployment: &Deployment,
        grace_period: Option<usize>,
    ) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.clone().unwrap();
        let ns = deployment
            .metadata
            .namespace
            .clone()
            .unwrap_or("default".to_string());

        let ids = self.execer.exec(
            "podman",
            &[
                "pod",
                "ls",
                "--filter",
                &format!("label=skate.io/namespace={}", ns),
                "--filter",
                &format!("label=skate.io/deployment={}", name),
                "-q",
            ],
            None,
        )?;

        let ids = ids
            .split("\n")
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<&str>>();

        self.pod_controller.delete_podman_pods(ids, grace_period)?;

        delete_resource(&self.db, &ResourceType::Deployment, &name, &ns).await?;
        Ok(())
    }
}
