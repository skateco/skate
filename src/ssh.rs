use anyhow::anyhow;
use async_ssh2_tokio::client::Client;
use async_ssh2_tokio::{AuthMethod, ServerCheckMethod};
use base64::engine::general_purpose;
use base64::Engine;
use std::error::Error;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::time::Duration;
use async_trait::async_trait;
use crate::config::{Cluster, Node};
use crate::skate::{Distribution, Platform};
use crate::skatelet::SystemInfo;
use crate::state::state::{NodeState, NodeStatus};
use colored::Colorize;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use itertools::Itertools;
use russh::{ChannelMsg, CryptoVec};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use crate::github;
use crate::resource::ResourceType;

#[async_trait]
pub trait SshClient: Send + Sync {
    async fn get_node_system_info(&self) -> Result<HostInfo, Box<dyn Error>>;
    async fn install_skatelet(&self, platform: Platform) -> Result<(), Box<dyn Error>>;
    async fn apply_resource(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>>;
    async fn remove_resource(&self, resource_type: ResourceType, name: &str, namespace: &str) -> Result<(String, String), Box<dyn Error>>;
    async fn remove_resource_by_manifest(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>>;
    async fn execute_stdout(&self, cmd: &str, print_command: bool, prefix_output: bool) -> Result<(), Box<dyn Error>>;
    // TODO-merge this into execute_stdout
    async fn execute_noisy(&self, cmd: &str) -> Result<String, Box<dyn Error>>;
    async fn execute(&self, cmd: &str) -> Result<String, Box<dyn Error>>;
    fn node_name(&self) -> String;
    
    async fn connect(n: &Node) -> Result<Self, SshError> where Self: Sized;
}

#[derive(Clone)]
pub struct RealSsh {
    pub node_name: String,
    pub client: Client,
}

impl Debug for RealSsh {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshClient").field("node_name", &self.node_name).finish()
    }
}

pub struct SshClients {
    pub clients: Vec<Box<dyn SshClient>>,
}



#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HostInfo {
    pub node_name: String,
    pub hostname: String,
    pub platform: Platform,
    pub skatelet_version: Option<String>,
    pub system_info: Option<SystemInfo>,
    pub podman_version: Option<String>,
    pub ovs_version: Option<String>,
}

impl From<HostInfo> for NodeState {
    fn from(val: HostInfo) -> Self {
        NodeState {
            node_name: val.node_name.to_string(),
            status: match val.healthy() {
                true => NodeStatus::Healthy,
                false => NodeStatus::Unhealthy
            },
            host_info: Some(val),
        }
    }
}

impl HostInfo {
    pub fn healthy(&self) -> bool {
        // TODO - actual checks for things that matter
        self.skatelet_version.is_some() && self.system_info.as_ref().map(|si| !si.cordoned).unwrap_or(false)
    }
}

impl RealSsh {


    fn print_crypto_vec(self: &RealSsh, data: &CryptoVec, prefix_output: bool, prev_last_char: char) -> char {
        let binding = data.to_vec();
        let s = String::from_utf8_lossy(&binding);
        if prefix_output && prev_last_char == '\n' {
            print!("{} | {}", self.node_name, s);
        } else {
            print!("{}", s);
        }
        if s.len() > 0 {
            s.chars().last().unwrap()
        } else {
            prev_last_char
        }
    }
}

#[async_trait]
impl SshClient for RealSsh {

    fn node_name(&self) ->String { self.node_name.clone()}

    async fn connect(node: &Node) -> Result<Self, SshError> {
        let default_key = "";
        let key = node.key.clone().unwrap_or(default_key.to_string());
        let key = shellexpand::tilde(&key).to_string();
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

        let ssh_client = result.map_err(|e| SshError{node_name: node.name.clone(), error: e.to_string()})?;

        Ok(RealSsh { node_name: node.name.clone(), client: ssh_client })
    }
    
    async fn get_node_system_info(&self) -> Result<HostInfo, Box<dyn Error>> {
        let command = r#"
hostname > /tmp/hostname-$$ &
arch > /tmp/arch-$$ &
uname -s > /tmp/os-$$ &
{ { cat /etc/issue |head -1|awk '{print $1}'; }  || echo '' ; } > /tmp/distro-$$ &
skatelet -V|awk '{print $NF}' > /tmp/skatelet-$$ &
podman --version|awk '{print $NF}' > /tmp/podman-$$ &
sudo skatelet system info|base64 -w0 > /tmp/sys-$$ &
ovs-vsctl --version|head -1| awk '{print $NF}' > /tmp/ovs-$$ &

wait;

echo hostname="$(cat /tmp/hostname-$$)";
echo arch="$(cat /tmp/arch-$$)";
echo os="$(cat /tmp/os-$$)";
echo distro="$(cat /tmp/distro-$$)";
echo skatelet="$(cat /tmp/skatelet-$$)";
echo podman="$(cat /tmp/podman-$$)";
echo sys="$(cat /tmp/sys-$$)";
echo ovs="$(cat /tmp/ovs-$$)";
"#;

        let result = self.client.execute(command).await?;

        if result.exit_status > 0 {
            let mut errlines = result.stderr.lines();
            return Err(anyhow!(errlines.join("\n")).into());
        }
        let lines = result.stdout.split("\n");
        let mut host_info = HostInfo {
            node_name: self.node_name.clone(),
            hostname: "".to_string(),
            platform: Platform {
                arch: "".to_string(),
                distribution: Distribution::Unknown,
            },
            skatelet_version: None,
            system_info: None,
            podman_version: None,
            ovs_version: None,
        };
        let mut arch: Option<String> = None;
        for line in lines {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "hostname" => host_info.hostname = v.to_string(),
                    "arch" => arch = Some(v.to_string()),
                    "distro" => host_info.platform.distribution = Distribution::from(v),
                    "skatelet" => host_info.skatelet_version = match v {
                        "" => None,
                        _ => v.to_string().strip_prefix("v").and_then(|v| Some(v.to_string()))
                    },
                    "podman" => host_info.podman_version = match v {
                        "" => None,
                        _ => Some(v.to_string())
                    },
                    "sys" => {
                        if !v.is_empty() {
                            match general_purpose::STANDARD.decode(v) {
                                Ok(v) => {
                                    if let Ok(sys_info) = serde_json::from_slice(&v) {
                                        host_info.system_info = sys_info
                                    }
                                }
                                Err(e) => {
                                    // TODO
                                }
                            };
                        }
                    }
                    "ovs" => host_info.ovs_version = match v {
                        "" => None,
                        _ => Some(v.to_string())
                    },
                    _ => {}
                }
            }
        }

        match &arch {
            Some(arch) => {
                host_info.platform.arch = arch.clone();
                host_info.system_info = host_info.system_info.map(|mut sys_info| {
                    sys_info.platform.arch = arch.clone();
                    sys_info
                })
            }
            None => {}
        }


        if host_info.skatelet_version.is_some() && host_info.system_info.is_none() {
            return Err(anyhow!("skatelet installed ({}) but failed to return system info", host_info.skatelet_version.unwrap()).into());
        }

        Ok(host_info)
    }
    async fn install_skatelet(&self, platform: Platform) -> Result<(), Box<dyn Error>> {

        let github_client = github::Client::new();
        
        let resp = github_client.get_latest_release().await?;

        let version = resp.version()?;
        
        let download_url = resp.find_skatelet_archive(&platform).ok_or(anyhow!("failed to find skatelet archive for platform"))?;

        println!("installing skatelet version {}", version);

        let cmd = format!("cd /tmp && wget {} -O skatelet.tar.gz && tar -xvf ./skatelet.tar.gz && chmod +x skatelet && sudo mv skatelet /usr/local/bin", download_url);
        self.execute_stdout(&cmd, true, true).await?;

        Ok(())
    }
    async fn apply_resource(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>> {
        let base64_manifest = general_purpose::STANDARD.encode(manifest);
        let result = self.client.execute(&format!("echo '{}'| base64 --decode|sudo skatelet apply -", base64_manifest)).await?;
        match result.exit_status {
            0 => {
                Ok((result.stdout.trim().to_string(), result.stderr.trim().to_string()))
            }
            _ => {
                let message = match result.stderr.len() {
                    0 => result.stdout.trim(),
                    _ => result.stderr.trim(),
                };
                Err(anyhow!("failed to apply resource: exit code {}, {}", result.exit_status, message).into())
            }
        }
    }
    async fn remove_resource(&self, resource_type: ResourceType, name: &str, namespace: &str) -> Result<(String, String), Box<dyn Error>> {
        let result = self.client.execute(&format!("sudo skatelet delete {} --name {} --namespace {}", resource_type.to_string().to_lowercase(), name, namespace)).await?;
        match result.exit_status {
            0 => {
                Ok((result.stdout, result.stderr))
            }
            _ => {
                let message = match result.stderr.len() {
                    0 => result.stdout,
                    _ => result.stderr
                };
                Err(anyhow!("{} - failed to remove resource: exit code {}, {}", self.node_name, result.exit_status, message.trim()).into())
            }
        }
    }
    async fn remove_resource_by_manifest(&self, manifest: &str) -> Result<(String, String), Box<dyn Error>> {
        let base64_manifest = general_purpose::STANDARD.encode(manifest);
        let result = self.client.execute(&format!("echo '{}' |base64  --decode|sudo skatelet delete -", base64_manifest)).await?;
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
    async fn execute_stdout(self: &RealSsh, cmd: &str, print_command: bool, prefix_output: bool) -> Result<(), Box<dyn Error>> {
        if print_command {
            cmd.lines().for_each(|l| println!("{} | > {}", self.node_name, l.green()));
        }

        let mut ch = self.client.get_channel().await?;
        ch.exec(true, cmd).await?;

        let mut result: Option<_> = None;
        let mut last_char = '\n';

        while let Some(msg) = ch.wait().await {
            //dbg!(&msg);
            match msg {
                // If we get data, add it to the buffer
                ChannelMsg::Data { ref data } => {
                    last_char = self.print_crypto_vec(data, prefix_output, last_char);
                }
                ChannelMsg::ExtendedData { ref data, ext } => {
                    if ext == 1 {
                        last_char = self.print_crypto_vec(data, prefix_output, last_char);
                    }
                }
                // If we get an exit code report, store it, but crucially don't
                // assume this message means end of communications. The data might
                // not be finished yet!
                ChannelMsg::ExitStatus { exit_status } => result = Some(exit_status),

                // We SHOULD get this EOF messagge, but 4254 sec 5.3 also permits
                // the channel to close without it being sent. And sometimes this
                // message can even precede the Data message, so don't handle it
                // russh::ChannelMsg::Eof => break,
                _ => {}
            }
        }

        if result.is_none() || result == Some(0) {
            return Ok(());
        }
        Err(anyhow!("exit status {}", result.unwrap()).into())
    }
    // TODO-merge this into execute_stdout
    async fn execute_noisy(self: &RealSsh, cmd: &str) -> Result<String, Box<dyn Error>> {
        cmd.lines().for_each(|l| println!("{} | > {}", self.node_name, l.green()));
        self.execute(cmd).await
    }
    async fn execute(self: &RealSsh, cmd: &str) -> Result<String, Box<dyn Error>> {
        let result = self.client.execute(cmd).await.
            map_err(|e| anyhow!(e).context(format!("{} failed", cmd)))?;
        if result.exit_status > 0 {
            return Err(anyhow!(result.stderr).context(format!("{} failed", cmd)).into());
        }
        if !result.stdout.is_empty() {
            // result.stdout.lines().for_each(|l| println!("{} |   {}", self.node_name, l));
        }
        Ok(result.stdout)
    }
}


#[derive(Error, Debug)]
pub struct SshError {
    pub node_name: String,
    pub error: String,
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

impl Error for SshErrors {}

impl fmt::Display for SshErrors {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let strs: Vec<String> = self.errors.iter().map(|ce| format!("{}", ce)).collect();
        write!(f, "{}", strs.join("\n"))
    }
}


impl Node {
    pub(crate) fn with_cluster_defaults(&self, cluster: &Cluster) -> Node {
        Node {
            name: self.name.clone(),
            host: self.host.clone(),
            subnet_cidr: self.subnet_cidr.clone(),
            port: self.port.or(Some(22)),
            user: self.user.clone().or(cluster.default_user.clone()),
            key: self.key.clone().or(cluster.default_key.clone()),
        }
    }
}



// pub async fn node_connection(cluster: &Cluster, node: &Node) -> Result<RealSsh, SshError> {
//     let node = node.with_cluster_defaults(cluster);
//     match connect_node(&node).await {
//         Ok(c) => Ok(c),
//         Err(err) => {
//             Err(SshError { node_name: node.name.clone(), error: err })
//         }
//     }
// }
// 
// pub async fn cluster_connections(cluster: &Cluster) -> (Option<RealSshClients>, Option<SshErrors>) {
//     let fut: FuturesUnordered<_> = cluster.nodes.iter().map(|n| node_connection(cluster, n)).collect();
// 
// 
//     let results: Vec<_> = fut.collect().await;
//     let (clients, errs): (Vec<RealSsh>, Vec<SshError>) = results.into_iter().partition_map(|r| match r {
//         Ok(client) => Either::Left(client),
//         Err(err) => Either::Right(err)
//     });
// 
// 
//     (
//         match clients.len() {
//             0 => None,
//             _ => Some(RealSshClients { clients })
//         },
//         match errs.len() {
//             0 => None,
//             _ => Some(SshErrors { errors: errs })
//         }
//     )
// }


impl SshClients {
    pub fn find(&self, node_name: &str) -> Option<&Box<dyn SshClient>> {
        self.clients.iter().find(|c| c.node_name() == node_name)
    }

    pub async fn execute(&self, command: &str) -> Vec<(String, Result<String, Box<dyn Error>>)> {
        let fut: FuturesUnordered<_> = self.clients.iter().map(|c| {
            c.execute(command)
        }).collect();
        let result: Vec<Result<_, _>> = fut.collect().await;

        result.into_iter().enumerate().map(|(i, r)| {
            let node_name = self.clients[i].node_name().clone();
            (node_name, r)
        }).collect()
    }

    pub async fn execute_noisy(&self, command: &str, args: &[&str]) -> Vec<(String, Result<String, Box<dyn Error>>)> {
        let concat_command = &format!("{} {}", &command, args.join(" "));
        let fut: FuturesUnordered<_> = self.clients.iter().map(|c| {
            c.execute_noisy(concat_command)
        }).collect();
        let result: Vec<Result<_, _>> = fut.collect().await;

        result.into_iter().enumerate().map(|(i, r)| {
            let node_name = self.clients[i].node_name().clone();
            (node_name, r)
        }).collect()
    }
    pub async fn get_nodes_system_info(&self) -> Vec<Result<HostInfo, Box<dyn Error>>> {
        let fut: FuturesUnordered<_> = self.clients.iter().map(|c| {
            c.get_node_system_info()
        }).collect();

        fut.collect().await
    }
}

