use std::error::Error;
use anyhow::anyhow;
use crate::controllers::ingress::IngressController;
use crate::filestore::Store;
use crate::spec::cert::ClusterIssuer;
use crate::util::metadata_name;

pub struct ClusterIssuerController {
    store: Box< dyn Store>,
    ingress_controller: IngressController,
}

impl ClusterIssuerController {
    pub fn new(store: Box<dyn Store>, ingress_controller: IngressController) -> Self {
        ClusterIssuerController {
            store,
            ingress_controller,
        }
        
    }


    pub fn apply(&self, cluster_issuer: &ClusterIssuer) -> Result<(), Box<dyn Error>> {
        // only thing special about this is must only have namespace 'skate'
        // and name 'default'
        let ingress_string = serde_yaml::to_string(cluster_issuer).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(cluster_issuer);
        // manifest goes into store
        self.store.write_file("clusterissuer", &ns_name.to_string(), "manifest.yaml", ingress_string.as_bytes())?;

        let hash = cluster_issuer.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("clusterissuer", &ns_name.to_string(), "hash", hash.as_bytes())?;
        }
        // need to retemplate nginx.conf
        self.ingress_controller.render_nginx_conf()?;
        self.ingress_controller.reload()?;

        Ok(())
    }
    pub fn delete(&self, cluster_issuer: &ClusterIssuer) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(cluster_issuer);


        let _ = self.store.remove_object("clusterissuer", &ns_name.to_string())?;

        // need to retemplate nginx.conf
        self.ingress_controller.render_nginx_conf()?;
        self.ingress_controller.reload()?;

        Ok(())
    }
}