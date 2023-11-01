use std::error::Error;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub current_context: Option<String>,
    pub clusters: Vec<Cluster>,
}

#[derive(Serialize, Deserialize)]
pub struct Cluster {
    pub name: String,
    pub default_user: Option<String>,
    pub default_key: Option<String>,
    pub nodes: Vec<Node>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Node {
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub key: Option<String>,
}

impl Config {
    pub fn current_cluster(&self) -> Result<&Cluster, Box<dyn Error>> {
        if self.clusters.len() == 0 {
            return Err(anyhow!("no clusters in config").into());
        }

        let first = &(self.clusters).first().expect("no first cluster");

        let cluster_name = self.current_context.clone().unwrap_or(first.name.to_owned());

        Ok(self.clusters.iter().find(|c| c.name == cluster_name)
            .expect(format!("found no cluster by name of {}", cluster_name).as_str()))
    }
}