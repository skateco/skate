use chrono::{DateTime, Utc};
use sqlx::SqliteExecutor;
use uuid::Uuid;

pub struct Resource {
    pub id: uuid::Uuid,
    pub name: String,
    pub namespace: String,
    pub resource_type: String,
    pub manifest: serde_json::Value,
    pub hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
pub async fn insert_resource(
    db: impl SqliteExecutor<'_>,
    resource: &Resource,
) -> super::Result<Uuid> {
    let resource_id = uuid::Uuid::new_v4();
    let str_id = resource_id.to_string();

    let _ = sqlx::query!(
        r#"
            INSERT INTO resources (
                id,
                name,
                namespace,
                type,
                manifest,
                hash
            ) VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        str_id,
        resource.name,
        resource.namespace,
        resource.resource_type,
        resource.manifest,
        resource.hash,
    )
    .execute(db)
    .await?;

    Ok(resource_id)
}
