use crate::skatelet::database::resource::Resource;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sqlx::SqliteExecutor;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Peer {
    pub id: i64,
    pub node_name: String,
    pub ip_address: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

pub async fn upsert_peer(db: impl SqliteExecutor<'_>, peer: &Peer) -> super::Result<()> {
    let _ = sqlx::query!(
        r#"
            INSERT INTO peers (
                node_name,
                ip_address
            ) VALUES ($1, $2)
            ON CONFLICT (node_name)
            DO UPDATE SET 
                ip_address = $2,
                updated_at = CURRENT_TIMESTAMP
        "#,
        peer.node_name,
        peer.ip_address,
    )
    .execute(db)
    .await?;
    Ok(())
}
