use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteExecutor};
use std::str::FromStr;
use strum_macros::{Display, EnumString};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub resource_type: ResourceType,
    pub manifest: serde_json::Value,
    pub hash: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

impl Default for Resource {
    fn default() -> Self {
        Resource {
            id: "".to_string(),
            name: "".to_string(),
            namespace: "".to_string(),
            resource_type: ResourceType::default(),
            manifest: serde_json::json!({}),
            hash: "".to_string(),
            created_at: Local::now(),
            updated_at: Local::now(),
        }
    }
}

pub async fn upsert_resource(
    db: impl SqliteExecutor<'_>,
    resource: &Resource,
) -> super::Result<()> {
    let resource_id = uuid::Uuid::new_v4().to_string();
    let str_id = resource_id.to_string();

    let _ = sqlx::query!(
        r#"
            INSERT INTO resources (
                id,
                name,
                namespace,
                resource_type,
                manifest,
                hash
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (resource_type, name, namespace)
            DO UPDATE SET 
                manifest = $5,
                hash = $6,
                updated_at = CURRENT_TIMESTAMP
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
    Ok(())
}

pub async fn delete_resource(
    db: impl SqliteExecutor<'_>,
    resource_type: &ResourceType,
    name: &str,
    namespace: &str,
) -> super::Result<()> {
    let _ = sqlx::query!(
        r#"
            DELETE FROM resources
            WHERE resource_type = $1
            AND name = $2
            AND namespace = $3
        "#,
        resource_type,
        name,
        namespace
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_resource(
    db: impl SqliteExecutor<'_>,
    resource_type: &ResourceType,
    name: &str,
    namespace: &str,
) -> super::Result<Option<Resource>> {
    let resource = sqlx::query_as!(
        Resource,
        r#" SELECT id as "id!: String", name as "name!: String", namespace as "namespace!: String", resource_type, manifest as "manifest!: serde_json::Value",  hash as "hash!: String", created_at as "created_at!: DateTime<Local>", updated_at as "updated_at!: DateTime<Local>"
            FROM resources
            WHERE resource_type = $1
            AND name = $2
            AND namespace = $3
        "#,
        resource_type,
        name,
        namespace
    )
    .fetch_optional(db)
    .await?;

    Ok(resource)
}
pub async fn list_resources_by_type(
    db: impl SqliteExecutor<'_>,
    resource_type: &ResourceType,
) -> super::Result<Vec<Resource>> {
    let resources = sqlx::query_as!(
        Resource,
        r#" SELECT id as "id!: String", name as "name!: String", namespace as "namespace!: String", resource_type, manifest as "manifest!: serde_json::Value",  hash as "hash!: String", created_at as "created_at!: DateTime<Local>", updated_at as "updated_at!: DateTime<Local>"
            FROM resources
            WHERE resource_type = $1
        "#,
        resource_type
    )
    .fetch_all(db)
    .await?;

    Ok(resources)
}

#[derive(
    sqlx::Type, Debug, Serialize, Deserialize, Display, Clone, EnumString, PartialEq, Default,
)]
#[strum(ascii_case_insensitive)]
pub enum ResourceType {
    #[default]
    #[strum(serialize = "pods", serialize = "pod", to_string = "pod")]
    Pod,
    #[strum(
        serialize = "deployments",
        serialize = "deployment",
        to_string = "deployment"
    )]
    Deployment,
    #[strum(
        serialize = "daemonsets",
        serialize = "daemonset",
        to_string = "daemonset"
    )]
    DaemonSet,
    #[strum(serialize = "ingress", to_string = "ingress")]
    Ingress,
    #[strum(serialize = "cronjobs", serialize = "cronjob", to_string = "cronjob")]
    CronJob,
    #[strum(serialize = "secrets", serialize = "secret", to_string = "secret")]
    Secret,
    #[strum(serialize = "services", serialize = "service", to_string = "service")]
    Service,
    #[strum(
        serialize = "clusterissuers",
        serialize = "clusterissuer",
        to_string = "clusterissuer"
    )]
    ClusterIssuer,
}

impl From<String> for ResourceType {
    fn from(s: String) -> Self {
        ResourceType::from_str(&s).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::skatelet::database::resource::ResourceType;

    #[test]
    fn test_resource_type_from_str() {
        let table = &[
            ("pod", ResourceType::Pod),
            ("pods", ResourceType::Pod),
            ("Pod", ResourceType::Pod),
            ("pods", ResourceType::Pod),
            ("daemonset", ResourceType::DaemonSet),
            ("daemonsets", ResourceType::DaemonSet),
            ("DaemonSet", ResourceType::DaemonSet),
            ("DaemonSets", ResourceType::DaemonSet),
        ];

        for (input, expect) in table {
            match ResourceType::from_str(input) {
                Ok(output) => {
                    assert_eq!(output, *expect, "input: {}", input);
                }
                Err(e) => {
                    panic!("{}: {}", *expect, e);
                }
            }
        }
    }
}
