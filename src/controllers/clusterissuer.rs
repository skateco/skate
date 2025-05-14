use crate::controllers::ingress::IngressController;
use crate::skatelet::database::resource::{
    delete_resource, upsert_resource, Resource, ResourceType,
};
use crate::spec::cert::ClusterIssuer;
use crate::util::metadata_name;
use anyhow::anyhow;
use sqlx::SqlitePool;
use std::error::Error;

pub struct ClusterIssuerController {
    ingress_controller: IngressController,
    db: SqlitePool,
}

impl ClusterIssuerController {
    pub fn new(db: SqlitePool, ingress_controller: IngressController) -> Self {
        ClusterIssuerController {
            ingress_controller,
            db,
        }
    }

    pub async fn apply(&self, cluster_issuer: &ClusterIssuer) -> Result<(), Box<dyn Error>> {
        // only thing special about this is must only have namespace 'skate'
        // and name 'default'
        let ingress_string = serde_yaml::to_string(cluster_issuer)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(cluster_issuer);

        let hash = cluster_issuer
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        let object = Resource {
            name: ns_name.name.clone(),
            namespace: ns_name.namespace.clone(),
            resource_type: ResourceType::ClusterIssuer,
            manifest: serde_json::to_value(cluster_issuer)?,
            hash,
            ..Default::default()
        };

        upsert_resource(&self.db, &object).await?;
        // need to retemplate nginx.conf
        self.ingress_controller.render_nginx_conf().await?;
        self.ingress_controller.reload()?;

        Ok(())
    }
    pub async fn delete(&self, cluster_issuer: &ClusterIssuer) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(cluster_issuer);

        delete_resource(
            &self.db,
            &ResourceType::ClusterIssuer,
            &ns_name.name,
            &ns_name.namespace,
        )
        .await?;

        // need to retemplate nginx.conf
        self.ingress_controller.render_nginx_conf().await?;
        self.ingress_controller.reload()?;

        Ok(())
    }
}
