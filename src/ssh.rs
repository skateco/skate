use std::error::Error;
use std::fmt;
use async_ssh2_tokio::client::{Client, CommandExecutedResult};
use futures::stream::FuturesUnordered;
use itertools::{Either, Itertools};
use crate::config::{Cluster, Node};
use crate::skate::{Distribution, Os, Platform};
use futures::StreamExt;


pub struct SshClient {
    pub node_name: String,
    pub client: Client,
}

pub struct SshClients {
    pub clients: Vec<SshClient>,
}

#[derive(Debug, Clone)]
pub struct HostInfoResponse {
    pub node_name: String,
    pub hostname: String,
    pub platform: Platform,
    pub skatelet_version: Option<String>,
}

impl SshClient {
    pub async fn get_host_info(&self) -> Result<HostInfoResponse, Box<dyn Error>> {
        let command = "\
hostname=`hostname`;
arch=`arch`;
os=`uname -s`;
distro=`cat /etc/issue|head -1|awk '{print $1}'`;
skatelet_version=`skatelet --version`;

echo $hostname;
echo $arch;
echo $os;
echo $distro;
echo $skatelet_version;
";

        let result = self.client.execute(command).await.expect("ssh command failed");

        let mut lines = result.stdout.lines();

        let hostname = lines.next().expect("missing hostname").to_string();
        let arch = lines.next().expect("missing arch").to_string();
        lines.next();
        let distro = Distribution::from(lines.next().map(String::from).unwrap_or_default());
        let skatelet_version = lines.next().map(String::from).filter(|s| !s.is_empty());
        ;

        return Ok(HostInfoResponse {
            node_name: self.node_name.clone(),
            hostname,
            platform: Platform {
                arch,
                os: Os::Unknown,
                distribution: distro,
            },
            skatelet_version,
        });
    }

    pub async fn download_skatelet(&self, platform: Platform) -> Result<(), Box<dyn Error>> {
        Ok(())
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


pub async fn connections(cluster: &Cluster) -> (Option<SshClients>, Option<SshErrors>) {
    let resolved_hosts = cluster.nodes.iter().map(|n| Node {
        name: n.name.clone(),
        host: n.host.clone(),
        port: n.port.or(Some(22)),
        user: n.user.clone().or(cluster.default_user.clone()),
        key: n.key.clone().or(cluster.default_key.clone()),
    });

    let fut: FuturesUnordered<_> = resolved_hosts.into_iter().map(|n| async move {
        match n.connect().await {
            Ok(c) => Ok(c),
            Err(err) => {
                Err(SshError { node_name: n.name.clone(), error: err.into() })
            }
        }
    }).collect();


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

impl SshClients {
    pub fn execute(&self, command: &str, args: &[&str]) -> Vec<(Node, Result<CommandExecutedResult, SshError>)> {
        todo!();
    }
    pub async fn get_hosts_info(&self) -> Vec<Result<HostInfoResponse, Box<dyn Error>>> {
        let fut: FuturesUnordered<_> = self.clients.iter().map(|c| {
            c.get_host_info()
        }).collect();

        fut.collect().await
    }
}
