use crate::config::{Cluster, Node};
use crate::deps::SshManager;
use crate::ssh::{SshClient, SshClients, SshError, SshErrors};
use async_trait::async_trait;

pub struct MockSshManager {}

#[async_trait]
impl SshManager for MockSshManager {
    async fn node_connect(&self, _: &Cluster, _: &Node) -> Result<Box<dyn SshClient>, SshError> {
        todo!("implement me")
    }

    async fn cluster_connect(&self, _: &Cluster) -> (Option<SshClients>, Option<SshErrors>) {
        todo!("implement me")
    }
}
