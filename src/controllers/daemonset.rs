use crate::controllers::pod::PodController;
use crate::skate::exec_cmd;
use k8s_openapi::api::apps::v1::DaemonSet;
use std::error::Error;
use crate::filestore::FileStore;
use crate::util::metadata_name;

pub struct DaemonSetController {
    store: FileStore,
}

impl DaemonSetController {
    pub fn new(store: FileStore) -> Self {
        DaemonSetController {
            store,
        }
    }

    pub fn apply(&self, ds: DaemonSet) -> Result<(), Box<dyn Error>> {
        self.store.write_file("daemonset", &metadata_name(&ds).to_string(), "manifest.yaml", serde_yaml::to_string(&ds)?.as_bytes())?;
        Ok(())
    }

    pub fn delete(&self, ds: DaemonSet, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let name = ds.metadata.name.clone().unwrap();
        let ns = ds.metadata.namespace.clone().unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/daemonset={}", name), "-q"])?;
        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        let pod_ctrl = PodController::new();

        pod_ctrl.delete_podman_pods(ids, grace_period)?;
        let _ = self.store.remove_object("daemonset", &metadata_name(&ds).to_string())?;
        Ok(())
    }
}