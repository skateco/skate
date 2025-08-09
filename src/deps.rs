use crate::config::{Cluster, Node};
use crate::exec::{RealExec, ShellExec};
use crate::node_client::{NodeClient, NodeClientError, NodeClientErrors, NodeClients, RealSsh};
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use itertools::{Either, Itertools};
use sqlx::SqlitePool;

pub trait With<T: ?Sized> {
    fn get(&self) -> Box<T>;
}

pub trait WithDB {
    fn get_db(&self) -> SqlitePool;
}

pub trait WithRef<'a, T: ?Sized> {
    fn get_ref(&'a self) -> &'a Box<T>;
}

pub struct SkateDeps {}

// impl<'a> WithRef<'a, dyn Store> for Deps {
//     fn get_ref(&'a self) -> &'a Box<dyn Store> {
//         &self.store
//     }
// }

impl With<dyn ShellExec> for SkateDeps {
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
    ) -> Result<Box<dyn NodeClient>, NodeClientError>;
    async fn cluster_connect(
        &self,
        cluster: &Cluster,
    ) -> (Option<NodeClients>, Option<NodeClientErrors>);
}

pub struct RealSshManager {}

impl RealSshManager {
    async fn _node_connect(
        cluster: &Cluster,
        node: &Node,
    ) -> Result<Box<dyn NodeClient>, NodeClientError> {
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
    ) -> Result<Box<dyn NodeClient>, NodeClientError> {
        Self::_node_connect(cluster, node).await
    }
    async fn cluster_connect(
        &self,
        cluster: &Cluster,
    ) -> (Option<NodeClients>, Option<NodeClientErrors>) {
        let fut: FuturesUnordered<_> = cluster
            .nodes
            .iter()
            .map(|n| Self::_node_connect(cluster, n))
            .collect();

        let results: Vec<_> = fut.collect().await;

        let (clients, errs): (Vec<_>, Vec<NodeClientError>) =
            results.into_iter().partition_map(|r| match r {
                Ok(client) => Either::Left(client),
                Err(err) => Either::Right(err),
            });

        (
            match clients.len() {
                0 => None,
                _ => {
                    let clients = NodeClients { clients };
                    Some(clients)
                }
            },
            match errs.len() {
                0 => None,
                _ => Some(NodeClientErrors { errors: errs }),
            },
        )
    }
}

impl With<dyn SshManager> for SkateDeps {
    fn get(&self) -> Box<dyn SshManager> {
        let m = RealSshManager {};
        Box::new(m)
    }
}
