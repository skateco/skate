use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqliteExecutor;
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Default)]
pub struct Resource {
    pub id: uuid::Uuid,
    pub name: String,
    pub namespace: String,
    pub resource_type: ResourceType,
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
                resource_type,
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
