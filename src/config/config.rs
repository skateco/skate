use crate::errors::SkateError;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::{create_dir, File};
use std::hash::Hash;
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub current_context: Option<String>,
    pub clusters: Vec<Cluster>,
}

#[derive(Serialize, Deserialize, Hash, Clone)]
pub struct Cluster {
    pub name: String,
    pub default_user: Option<String>,
    pub default_key: Option<String>,
    pub nodes: Vec<Node>,
}

fn default_string() -> String {
    "".to_string()
}
#[derive(Serialize, Deserialize, Clone, Debug, Hash, Default)]
pub struct Node {
    pub name: String,
    pub host: String,
    #[serde(default = "default_string")]
    pub peer_host: String,
    pub subnet_cidr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl Config {
    pub fn active_cluster(&self, name: Option<String>) -> Result<&Cluster, SkateError> {
        if self.clusters.is_empty() {
            return Err(anyhow!("no clusters in config").into());
        }

        let first = &(self.clusters)
            .first()
            .ok_or("no first cluster".to_string())?;

        let cluster_name = self
            .current_context
            .clone()
            .unwrap_or(name.unwrap_or(first.name.to_owned()));

        let cluster = self
            .clusters
            .iter()
            .find(|c| c.name == cluster_name)
            .ok_or(format!("found no cluster by name of {}", cluster_name))?;
        Ok(cluster)
    }

    pub fn replace_cluster(&mut self, cluster: &Cluster) -> Result<(), SkateError> {
        let idx = self
            .clusters
            .iter()
            .position(|c| c.name == cluster.name)
            .ok_or("cluster not found".to_string())?;
        self.clusters[idx] = cluster.clone();
        Ok(())
    }

    pub fn delete_cluster(&mut self, cluster: &Cluster) -> Result<(), SkateError> {
        let idx = self
            .clusters
            .iter()
            .position(|c| c.name == cluster.name)
            .ok_or("cluster not found".to_string())?;
        self.clusters.remove(idx);
        Ok(())
    }
}

pub fn config_dir() -> String {
    shellexpand::tilde("~/.skate").to_string()
}

pub fn cache_dir() -> String {
    config_dir() + "/cache"
}

pub fn ensure_config() -> Result<(), SkateError> {
    let dir = config_dir();
    let path = Path::new(&dir);
    if !path.exists() {
        create_dir(path).expect("couldn't create skate config path")
    }

    let dir = cache_dir();
    let cache_path = Path::new(&dir);
    if !cache_path.exists() {
        create_dir(cache_path).expect("couldn't create skate cache path")
    }

    let path = path.join("config.yaml");

    let default_config = Config {
        current_context: None,
        clusters: vec![],
    };

    if !path.exists() {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| anyhow!(e).context("couldn't open config file"))?;
        serde_yaml::to_writer(f, &default_config)?;
    }

    Ok(())
}

impl Config {
    fn path(path: Option<String>) -> String {
        let path = match path {
            Some(path) => path,
            None => config_dir() + "/config.yaml",
        };
        shellexpand::tilde(&path).to_string()
    }
    pub fn load(path: Option<String>) -> Result<Config, SkateError> {
        let path = Config::path(path);
        let path = Path::new(&path);
        let f =
            fs::File::open(path).map_err(|e| anyhow!(e).context("failed to open config file"))?;
        let mut data: Config = serde_yaml::from_reader(f)
            .map_err(|e| anyhow!(e).context("failed to read config file"))?;
        data.enrich();
        Ok(data)
    }

    fn enrich(&mut self) {
        self.clusters.iter_mut().for_each(|c| {
            c.nodes.iter_mut().for_each(|n| {
                if n.peer_host.is_empty() {
                    n.peer_host = n.host.clone();
                }
            });
        });
    }

    pub fn persist(&self, path: Option<String>) -> Result<(), SkateError> {
        let path = Config::path(path);
        let state_file = File::create(Path::new(&path))
            .map_err(|e| anyhow!(e).context("unable to read config file"))?;
        serde_yaml::to_writer(state_file, self)
            .map_err(|e| anyhow!(e).context("failed to write config file"))?;
        Ok(())
    }
}
