use crate::controllers::pod::PodController;
use crate::exec::ShellExec;
use crate::skatelet::database::resource;
use crate::util::{SkateLabels, get_skate_label_value, metadata_name};
use k8s_openapi::api::apps::v1::DaemonSet;
use sqlx::SqlitePool;
use std::error::Error;

pub struct DaemonSetController {
    db: SqlitePool,
    execer: Box<dyn ShellExec>,
    pod_controller: PodController,
}

impl DaemonSetController {
    pub fn new(db: SqlitePool, execer: Box<dyn ShellExec>, pod_controller: PodController) -> Self {
        DaemonSetController {
            db,
            execer,
            pod_controller,
        }
    }

    pub async fn apply(&self, ds: &DaemonSet) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(ds);
        let hash = get_skate_label_value(&ds.metadata.labels, &SkateLabels::Hash)
            .unwrap_or("".to_string());

        let generation = ds.metadata.generation.unwrap_or_default();

        let object = resource::Resource {
            name: ns_name.name.clone(),
            namespace: ns_name.namespace.clone(),
            resource_type: resource::ResourceType::DaemonSet,
            manifest: serde_json::to_value(ds)?,
            hash: hash.clone(),
            generation,
            ..Default::default()
        };
        resource::upsert_resource(&self.db, &object).await?;
        Ok(())
    }

    pub async fn delete(
        &self,
        ds: &DaemonSet,
        grace_period: Option<usize>,
    ) -> Result<(), Box<dyn Error>> {
        let name = ds.metadata.name.clone().unwrap();
        let ns = ds
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
                &format!("label=skate.io/daemonset={}", name),
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

        resource::delete_resource(&self.db, &resource::ResourceType::DaemonSet, &name, &ns).await?;
        Ok(())
    }
}
