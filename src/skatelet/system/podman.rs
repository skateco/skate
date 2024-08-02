use std::collections::HashMap;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecret {
    #[serde(rename = "ID")]
    pub id: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub spec: PodmanSecretSpec,
    pub secret_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecretSpec {
    pub name: String,
    pub driver: PodmanSecretDriver,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PodmanSecretDriver {
    pub name: String,
    pub options: HashMap<String, String>,
}