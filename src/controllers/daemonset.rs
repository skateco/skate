use crate::controllers::pod::PodController;
use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::util::metadata_name;
use k8s_openapi::api::apps::v1::DaemonSet;
use std::error::Error;

pub struct DaemonSetController {
    store: Box<dyn Store>,
    execer: Box<dyn ShellExec>,
    pod_controller: PodController
}

impl DaemonSetController {
    pub fn new(store: Box<dyn Store>, execer: Box<dyn ShellExec>, pod_controller: PodController) -> Self {
        DaemonSetController {
            store,
            execer,
            pod_controller,
        }
    }

    pub fn apply(&self, ds: &DaemonSet) -> Result<(), Box<dyn Error>> {
        
        self.store.write_file("daemonset", &metadata_name(ds).to_string(), "manifest.yaml", serde_yaml::to_string(&ds)?.as_bytes())?;

        let ns_name = metadata_name(ds);
        let hash = ds.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();
        if !hash.is_empty() {
            self.store.write_file("daemonset", &ns_name.to_string(), "hash", hash.as_bytes())?;
        }
        Ok(())
    }

    pub fn delete(&self, ds: &DaemonSet, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let name = ds.metadata.name.clone().unwrap();
        let ns = ds.metadata.namespace.clone().unwrap_or("default".to_string());

        let ids = self.execer.exec("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/daemonset={}", name), "-q"])?;
        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();


        self.pod_controller.delete_podman_pods(ids, grace_period)?;
        let _ = self.store.remove_object("daemonset", &metadata_name(ds).to_string())?;
        Ok(())
    }
}