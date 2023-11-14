use std::error::Error;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::time::Duration;
use anyhow::anyhow;
use async_ssh2_tokio::{AuthMethod, ServerCheckMethod};
use async_ssh2_tokio::client::{Client, CommandExecutedResult};
use futures::stream::FuturesUnordered;
use itertools::{Either, Itertools};
use crate::config::{Cluster, Node};
use crate::skate::{Distribution, Os, Platform};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use crate::skatelet::SystemInfo;
use crate::state::state::{NodeState, NodeStatus};
use crate::util::hash_string;


pub struct SshClient {
    pub node_name: String,
    pub client: Client,
}

impl Debug for SshClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshClient").field("node_name", &self.node_name).finish()
    }
}

pub struct SshClients {
    pub clients: Vec<SshClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSystemInfo {
    pub node_name: String,
    pub hostname: String,
    pub platform: Platform,
    pub skatelet_version: Option<String>,
    pub system_info: Option<SystemInfo>,
    pub podman_version: Option<String>,
}

impl Into<NodeState> for NodeSystemInfo {
    fn into(self) -> NodeState {
        NodeState{
            node_name: self.node_name.to_string(),
            status: match self.healthy() {
                true => NodeStatus::Healthy,
                false => NodeStatus::Unhealthy
            },
            host_info: Some(self),
        }

    }
}

impl NodeSystemInfo {
    pub fn healthy(&self) -> bool {
        // TODO - actual checks for things that matter
        self.skatelet_version.is_some()
    }
}

impl SshClient {
    pub async fn get_node_system_info(&self) -> Result<NodeSystemInfo, Box<dyn Error>> {
        let command = "\
hostname > /tmp/hostname-$$ &
arch > /tmp/arch-$$ &
uname -s > /tmp/os-$$ &
{ cat /etc/issue |head -1|awk '{print $1}'; } || echo '' > /tmp/distro-$$ &
skatelet --version|awk '{print $NF}' > /tmp/skatelet-$$ &
podman --version|awk '{print $NF}' > /tmp/podman-$$ &
skatelet system info > /tmp/sys-$$ &

wait;

echo hostname=$(cat /tmp/hostname-$$);
echo arch=$(cat /tmp/arch-$$);
echo os=$(cat /tmp/os-$$);
echo distro=$(cat /tmp/distro-$$);
echo skatelet=$(cat /tmp/skatelet-$$);
echo podman=$(cat /tmp/podman-$$);
echo sys=$(cat /tmp/sys-$$);
";

        let result = self.client.execute(command).await?;

        if result.exit_status > 0 {
            let mut errlines = result.stderr.lines();
            return Err(anyhow!(errlines.join("\n")).into());
        }
        let lines = result.stdout.lines();
        let mut host_info = NodeSystemInfo {
            node_name: self.node_name.clone(),
            hostname: "".to_string(),
            platform: Platform {
                arch: "".to_string(),
                os: Os::Unknown,
                distribution: Distribution::Unknown,
            },
            skatelet_version: None,
            system_info: None,
            podman_version: None,
        };

        let mut arch: Option<String> = None;
        for line in lines {
            match line.split_once('=') {
                Some((k, v)) => {
                    match k {
                        "hostname" => host_info.hostname = v.to_string(),
                        "arch" => arch = Some(v.to_string()),
                        "os" => host_info.platform.os = Os::from_str_loose(v),
                        "distro" => host_info.platform.distribution = Distribution::from(v.to_string()),
                        "skatelet" => host_info.skatelet_version = Some(v.to_string()),
                        "podman" => host_info.podman_version = Some(v.to_string()),
                        "sys" => {
                            match serde_json::from_str(v) {
                                Ok(sys_info) => host_info.system_info = sys_info,
                                Err(_) => {}
                            }
                        }
                        _ => {}
                    }
                }
                None => {}
            }
        }

        match arch {
            Some(arch) => host_info.platform.arch = arch,
            None => {}
        }


        if host_info.skatelet_version.is_some() && host_info.system_info.is_none() {
            return Err(anyhow!("skatelet installed but failed to return system info").into());
        }

        Ok(host_info)
    }

    pub async fn install_skatelet(&self, _platform: Platform) -> Result<(), Box<dyn Error>> {

        // TODO - download from bucket etc

        let _ = self.client.execute(format!("sudo mv /tmp/skatelet /usr/local/bin/skatelet && sudo chmod +x /usr/local/bin/skatelet")
            .as_str()).await.expect("failed to fetch binary");

        Ok(())
    }
    pub async fn apply_resource(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>> {
        let hash = hash_string(manifest);
        let file_name = format!("/tmp/skate-{}.yaml", hash);
        let result = self.client.execute(&format!("echo \"{}\" > {} && \
        cat {} | skatelet apply -", manifest, file_name, file_name)).await?;
        match result.exit_status {
            0 => {
                Ok((result.stdout, result.stderr))
            }
            _ => {
                let message = match result.stderr.len() {
                    0 => result.stdout,
                    _ => result.stderr
                };
                Err(anyhow!("failed to apply resource: exit code {}, {}", result.exit_status, message).into())
            }
        }
    }

    pub async fn remove_resource(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>> {
        let hash = hash_string(manifest);
        let file_name = format!("/tmp/skate-{}.yaml", hash);
        let result = self.client.execute(&format!("echo \"{}\" > {} && \
        cat {} | skatelet remove -", manifest, file_name, file_name)).await?;
        match result.exit_status {
            0 => {
                Ok((result.stdout, result.stderr))
            }
            _ => {
                let message = match result.stderr.len() {
                    0 => result.stdout,
                    _ => result.stderr
                };
                Err(anyhow!("failed to remove resource: exit code {}, {}", result.exit_status, message).into())
            }
        }
    }
}


#[derive(Debug)]
pub struct SshError {
    pub node_name: String,
    pub error: Box<dyn Error>,
}

impl fmt::Display for SshError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.node_name, self.error)
    }
}

#[derive(Debug)]
pub struct SshErrors {
    pub errors: Vec<SshError>,
}

impl fmt::Display for SshErrors {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let strs: Vec<String> = self.errors.iter().map(|ce| format!("{}", ce)).collect();
        write!(f, "{}", strs.join("\n"))
    }
}


impl Node {
    fn with_cluster_defaults(&self, cluster: &Cluster) -> Node {
        Node {
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port.or(Some(22)),
            user: self.user.clone().or(cluster.default_user.clone()),
            key: self.key.clone().or(cluster.default_key.clone()),
        }
    }
}

pub async fn node_connection(cluster: &Cluster, node: &Node) -> Result<SshClient, SshError> {
    let node = node.with_cluster_defaults(cluster);
    match connect_node(&node).await {
        Ok(c) => Ok(c),
        Err(err) => {
            Err(SshError { node_name: node.name.clone(), error: err.into() })
        }
    }
}

pub async fn cluster_connections(cluster: &Cluster) -> (Option<SshClients>, Option<SshErrors>) {
    let fut: FuturesUnordered<_> = cluster.nodes.iter().map(|n| node_connection(cluster, n)).collect();


    let results: Vec<_> = fut.collect().await;
    let (clients, errs): (Vec<SshClient>, Vec<SshError>) = results.into_iter().partition_map(|r| match r {
        Ok(client) => Either::Left(client),
        Err(err) => Either::Right(err)
    });


    return (
        match clients.len() {
            0 => None,
            _ => Some(SshClients { clients })
        },
        match errs.len() {
            0 => None,
            _ => Some(SshErrors { errors: errs })
        });
}

async fn connect_node(node: &Node) -> Result<SshClient, Box<dyn Error>> {
    let default_key = "";
    let key = node.key.clone().unwrap_or(default_key.to_string());
    let key = shellexpand::tilde(&key);
    let timeout = Duration::from_secs(5);

    let auth_method = AuthMethod::with_key_file(&key, None);
    let result = tokio::time::timeout(timeout, Client::connect(
        (&*node.host, node.port.unwrap_or(22)),
        node.user.clone().unwrap_or(String::from("")).as_str(),
        auth_method,
        ServerCheckMethod::NoCheck,
    )).await;

    let result: Result<_, Box<dyn Error>> = match result {
        Ok(r2) => r2.map_err(|e| e.into()),
        _ => Err(anyhow!("timeout").into())
    };

    let ssh_client = result?;

    Ok(SshClient { node_name: node.name.clone(), client: ssh_client })
}

impl SshClients {
    pub fn find(&self, node_name: &str) -> Option<&SshClient> {
        self.clients.iter().find(|c| c.node_name == node_name)
    }
    pub fn execute(&self, _command: &str, _args: &[&str]) -> Vec<(Node, Result<CommandExecutedResult, SshError>)> {
        todo!();
    }
    pub async fn get_nodes_system_info(&self) -> Vec<Result<NodeSystemInfo, Box<dyn Error>>> {
        let fut: FuturesUnordered<_> = self.clients.iter().map(|c| {
            c.get_node_system_info()
        }).collect();

        fut.collect().await
    }
}
