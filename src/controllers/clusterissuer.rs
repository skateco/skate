use std::error::Error;
use anyhow::anyhow;
use crate::controllers::ingress::IngressController;
use crate::filestore::FileStore;
use crate::spec::cert::ClusterIssuer;
use crate::util::metadata_name;

pub struct ClusterIssuerController {
    store: FileStore,
}

impl ClusterIssuerController {
    pub fn new(file_store: FileStore) -> Self {
        ClusterIssuerController {
            store: file_store,
        }
    }


    pub fn apply(&self, cluster_issuer: ClusterIssuer) -> Result<(), Box<dyn Error>> {
        // only thing special about this is must only have namespace 'skate'
        // and name 'default'
        let ingress_string = serde_yaml::to_string(&cluster_issuer).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(&cluster_issuer);
        // manifest goes into store
        self.store.write_file("clusterissuer", &ns_name.to_string(), "manifest.yaml", ingress_string.as_bytes())?;

        let hash = cluster_issuer.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("clusterissuer", &ns_name.to_string(), "hash", &hash.as_bytes())?;
        }
        // need to retemplate nginx.conf
        let ingress_ctrl = IngressController::new(self.store.clone());
        ingress_ctrl.render_nginx_conf()?;
        IngressController::reload()?;

        Ok(())
    }
    pub fn delete(&self, cluster_issuer: ClusterIssuer) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&cluster_issuer);


        let _ = self.store.remove_object("clusterissuer", &ns_name.to_string())?;

        // need to retemplate nginx.conf
        let ingress_ctrl = IngressController::new(self.store.clone());
        ingress_ctrl.render_nginx_conf()?;
        IngressController::reload()?;

        Ok(())
    }
}