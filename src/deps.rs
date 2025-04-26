use crate::config::{Cluster, Node};
use crate::exec::{RealExec, ShellExec};
use crate::filestore::{FileStore, Store};
use crate::skatelet::VAR_PATH;
use crate::ssh::{RealSsh, SshClient, SshClients, SshError, SshErrors};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use itertools::{Either, Itertools};
use sqlx::SqliteConnection;

pub trait With<T: ?Sized> {
    fn get(&self) -> Box<T>;
}

pub trait WithDB {
    fn get_db(&self) -> &SqliteConnection;
}

pub trait WithRef<'a, T: ?Sized> {
    fn get_ref(&'a self) -> &'a Box<T>;
}

pub struct Deps {
    pub db: SqliteConnection,
}

impl WithDB for Deps {
    fn get_db(&self) -> &SqliteConnection {
        &self.db
    }
}

impl With<dyn Store> for Deps {
    fn get(&self) -> Box<dyn Store> {
        Box::new(FileStore::new(format!("{}/store", VAR_PATH)))
    }
}

// impl<'a> WithRef<'a, dyn Store> for Deps {
//     fn get_ref(&'a self) -> &'a Box<dyn Store> {
//         &self.store
//     }
// }

impl With<dyn ShellExec> for Deps {
    fn get(&self) -> Box<dyn ShellExec> {
        Box::new(RealExec {})
    }
}

#[async_trait]
pub trait SshManager {
    async fn node_connect(
        &self,
        cluster: &Cluster,
        node: &Node,
    ) -> Result<Box<dyn SshClient>, SshError>;
    async fn cluster_connect(&self, cluster: &Cluster) -> (Option<SshClients>, Option<SshErrors>);
}

pub struct RealSshManager {}

impl RealSshManager {
    async fn _node_connect(cluster: &Cluster, node: &Node) -> Result<Box<dyn SshClient>, SshError> {
        let node = node.with_cluster_defaults(cluster);
        match RealSsh::connect(&node).await {
            Ok(c) => Ok(Box::new(c)),
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl SshManager for RealSshManager {
    async fn node_connect(
        &self,
        cluster: &Cluster,
        node: &Node,
    ) -> Result<Box<dyn SshClient>, SshError> {
        Self::_node_connect(cluster, node).await
    }
    async fn cluster_connect(&self, cluster: &Cluster) -> (Option<SshClients>, Option<SshErrors>) {
        let fut: FuturesUnordered<_> = cluster
            .nodes
            .iter()
            .map(|n| Self::_node_connect(cluster, n))
            .collect();

        let results: Vec<_> = fut.collect().await;

        let (clients, errs): (Vec<_>, Vec<SshError>) =
            results.into_iter().partition_map(|r| match r {
                Ok(client) => Either::Left(client),
                Err(err) => Either::Right(err),
            });

        (
            match clients.len() {
                0 => None,
                _ => {
                    let clients = SshClients { clients };
                    Some(clients)
                }
            },
            match errs.len() {
                0 => None,
                _ => Some(SshErrors { errors: errs }),
            },
        )
    }
}

impl With<dyn SshManager> for Deps {
    fn get(&self) -> Box<dyn SshManager> {
        let m = RealSshManager {};
        Box::new(m)
    }
}
