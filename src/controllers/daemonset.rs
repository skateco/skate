use crate::controllers::pod::PodController;
use crate::filestore::FileStore;
use crate::skate::exec_cmd;
use k8s_openapi::api::apps::v1::DaemonSet;
use std::error::Error;

pub struct DaemonSetController {
    store: FileStore,
}

impl DaemonSetController {
    pub fn new(file_store: FileStore) -> Self {
        DaemonSetController {
            store: file_store,
        }
    }

    pub fn delete(&self, ds: DaemonSet, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
            let name = ds.metadata.name.unwrap();
            let ns = ds.metadata.namespace.unwrap_or("default".to_string());

            let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/daemonset={}", name), "-q"])?;
            let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

            let  pod_ctrl = PodController::new();
            pod_ctrl.delete_podman_pods(ids, grace_period)
    }
}