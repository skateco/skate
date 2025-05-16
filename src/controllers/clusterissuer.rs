use crate::controllers::ingress::IngressController;
use crate::skatelet::database::resource::{
    delete_resource, upsert_resource, Resource, ResourceType,
};
use crate::spec::cert::ClusterIssuer;
use crate::util::{get_skate_label_value, metadata_name, SkateLabels};
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

        let ns_name = metadata_name(cluster_issuer);

        let hash = get_skate_label_value(&cluster_issuer.metadata.labels, &SkateLabels::Hash)
            .unwrap_or("".to_string());

        let generation = cluster_issuer.metadata.generation.unwrap_or_default();

        let object = Resource {
            name: ns_name.name.clone(),
            namespace: ns_name.namespace.clone(),
            resource_type: ResourceType::ClusterIssuer,
            manifest: serde_json::to_value(cluster_issuer)?,
            hash,
            generation,
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
