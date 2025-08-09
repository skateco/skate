use crate::config::{Cluster, Node};
use crate::deps::SshManager;
use crate::node_client::{NodeClient, NodeClientError, NodeClientErrors, NodeClients};
use async_trait::async_trait;

pub struct MockSshManager {}

#[async_trait]
impl SshManager for MockSshManager {
    async fn node_connect(
        &self,
        _: &Cluster,
        _: &Node,
    ) -> Result<Box<dyn NodeClient>, NodeClientError> {
        todo!("implement me")
    }

    async fn cluster_connect(
        &self,
        _: &Cluster,
    ) -> (Option<NodeClients>, Option<NodeClientErrors>) {
        todo!("implement me")
    }
}
