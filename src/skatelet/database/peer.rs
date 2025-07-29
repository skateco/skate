use crate::skatelet::database::resource::{Resource, ResourceType};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sqlx::SqliteExecutor;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Peer {
    pub id: i64,
    pub node_name: String,
    pub host: String,
    pub subnet_cidr: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

pub async fn upsert_peer(db: impl SqliteExecutor<'_>, peer: &Peer) -> super::Result<()> {
    let _ = sqlx::query!(
        r#"
            INSERT INTO peers (
                node_name,
                host,
                subnet_cidr
            ) VALUES ($1, $2, $3)
            ON CONFLICT (node_name)
            DO UPDATE SET 
                host = $2,
                subnet_cidr = $3,
                updated_at = CURRENT_TIMESTAMP
        "#,
        peer.node_name,
        peer.host,
        peer.subnet_cidr,
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_peers(db: impl SqliteExecutor<'_>) -> super::Result<Vec<Peer>> {
    let peers = sqlx::query_as!(
        Peer,
        r#" SELECT id as "id!: i64", node_name as "node_name!: String", host as "host!: String", subnet_cidr as "subnet_cidr!: String",  created_at as "created_at!: DateTime<Local>", updated_at as "updated_at!: DateTime<Local>"
            FROM peers
        "#,
    )
        .fetch_all(db)
        .await?;

    Ok(peers)
}
