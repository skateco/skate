use std::error::Error;
use std::net::ToSocketAddrs;
use anyhow::anyhow;
use base64::Engine;
use base64::engine::general_purpose;
use clap::{Args, Subcommand};
use itertools::{Itertools, min};
use semver::{Version, VersionReq};
use crate::config::{Cluster, Config, Node};
use crate::skate::{ConfigFileArgs, Distribution, Os};
use crate::ssh::{cluster_connections, node_connection, NodeSystemInfo, SshClient};
use crate::state::state::{ClusterState, NodeState, NodeStatus};
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: CreateCommands,
}

#[derive(Debug, Subcommand)]
pub enum CreateCommands {
    Node(CreateNodeArgs),
}

#[derive(Debug, Args)]
pub struct CreateNodeArgs {
    #[arg(long, long_help = "Name of the node.")]
    name: String,
    #[arg(long, long_help = "IP or domain name of the node.")]
    host: String,
    #[arg(long, long_help = "Ssh user for connecting")]
    user: Option<String>,
    #[arg(long, long_help = "Ssh key for connecting")]
    key: Option<String>,
    #[arg(long, long_help = "Ssh port for connecting")]
    port: Option<u16>,
    #[arg(long, long_help = "Subnet cidr for podman network (must be unique range per host)")]
    subnet_cidr: String,

    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn create(args: CreateArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        CreateCommands::Node(args) => create_node(args).await?
    }
    Ok(())
}

async fn create_node(args: CreateNodeArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;


    let context = match args.config.context {
        None => match config.current_context {
            None => {
                Err(anyhow!("--cluster is required unless there is already a current context"))
            }
            Some(ref context) => Ok(context)
        }
        Some(ref context) => Ok(context)
    }.map_err(Into::<Box<dyn Error>>::into)?;

    let (cluster_index, cluster) = config.clusters.iter().find_position(|c| c.name == context.clone()).ok_or(anyhow!("no cluster by name of {}", context))?;
    let mut cluster = (*cluster).clone();

    let mut state = ClusterState::load(cluster.name.as_str())?;

    let mut nodes_iter = cluster.nodes.clone().into_iter();

    let existing_node = nodes_iter.find(|n| n.name == args.name || n.host == args.host);

    let (new, node) = match existing_node {
        Some(node) => (false, node.clone()),
        None => {
            let node = Node {
                name: args.name.clone(),
                host: args.host,
                port: args.port,
                user: args.user,
                key: args.key,
                subnet_cidr: args.subnet_cidr,
            };
            cluster.nodes.push(node.clone());
            (true, node)
        }
    };

    let conn = node_connection(&cluster, &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;
    let info = conn.get_node_system_info().await?;

    println!("{:}", &info.platform);

    match &(info.platform).os {
        Os::Linux => {}
        _ => {
            return Err(anyhow!("detected os {}: only linux is supported", &(info.platform).os).into());
        }
    }

    match info.skatelet_version.as_ref() {
        None => {
            // install skatelet
            let _ = conn.install_skatelet(info.platform.clone()).await?;
        }
        Some(v) => {
            println!("skatelet version {} already installed {} ", v, CHECKBOX_EMOJI)
        }
    }

    match info.podman_version.as_ref() {
        Some(version) => {
            let min_podman_ver = ">=3.0.0";
            let req = VersionReq::parse(min_podman_ver).unwrap();
            let version = Version::parse(&version).unwrap();

            if !req.matches(&version) {
                return Err(anyhow!("podman version too old, must be {}, see https://podman.io/docs/installation", min_podman_ver).into());
            }
            println!("podman version {} already installed {} ", version, CHECKBOX_EMOJI)
        }
        // instruct on installing newer podman version
        None => {
            let installed = match info.platform.clone().os {
                Os::Linux => {
                    match info.platform.distribution {
                        Distribution::Unknown => false,
                        Distribution::Debian | Distribution::Raspbian => {
                            let command = "sh -c 'sudo apt-get -y update && sudo apt-get install -y podman'";
                            println!("installing podman with command {}", command);
                            let result = conn.client.execute(command).await?;
                            if result.exit_status > 0 {
                                let mut lines = result.stderr.lines();
                                println!("failed to install podman {} :\n{}", CROSS_EMOJI, lines.join("\n"));
                                false
                            } else {
                                println!("podman installed successfully {} ", CHECKBOX_EMOJI);
                                true
                            }
                        }
                    }
                }
                _ => false
            };
            if !installed {
                return Err(anyhow!("podman not installed, see https://podman.io/docs/installation").into());
            }
        }
    }

    setup_networking(&conn, &cluster, &node, &info).await?;

    state.reconcile_node(&info)?;


    config.clusters[cluster_index] = cluster;

    config.persist(Some(args.config.skateconfig))?;

    state.persist()?;

    Ok(())
}

async fn execute(conn: &SshClient, cmd: &str) -> Result<String, Box<dyn Error>> {
    println!(">>> {}", cmd);
    let result = conn.client.execute(cmd).await.
        map_err(|e| anyhow!("{} failed", cmd).context(e))?;
    if result.exit_status > 0 {
        return Err(anyhow!("{} failed", cmd).context(result.stderr).into());
    }
    println!("{}", result.stdout);
    Ok(result.stdout)
}

async fn setup_networking(conn: &SshClient, cluster_conf: &Cluster, node: &Node, info: &NodeSystemInfo) -> Result<(), Box<dyn Error>> {
    let cmd = "sudo cp /usr/share/containers/containers.conf /etc/containers/containers.conf";
    execute(conn, cmd).await?;

    let cmd = format!("sudo sed -i 's&#default_subnet.*&default_subnet = \"{}\"&' /etc/containers/containers.conf", node.subnet_cidr);
    execute(conn, &cmd).await?;

    let gateway = node.subnet_cidr.split(".").take(3).join(".") + ".1";
    let cni = "
{
  \"cniVersion\": \"0.4.0\",
  \"name\": \"podman\",
  \"plugins\": [
    {
      \"type\": \"bridge\",
      \"bridge\": \"cni-podman0\",
      \"isGateway\": true,
      \"ipMasq\": true,
      \"hairpinMode\": true,
      \"ipam\": {
        \"type\": \"host-local\",
        \"routes\": [
                { \"dst\": \"0.0.0.0/0\" }
        ],
        \"ranges\": [
          [
            {
              \"subnet\": \"%%subnet%%\",
              \"gateway\": \"%%gateway%%\"
            }
          ]
        ]
      }
    },
    {
      \"type\": \"portmap\",
      \"capabilities\": {
        \"portMappings\": true
      }
    },
    {
      \"type\": \"firewall\"
    },
    {
      \"type\": \"tuning\"
    }
  ]
}\n".replace("%%subnet%%", &node.subnet_cidr).replace("%%gateway%%", &gateway);

    let cni = general_purpose::STANDARD.encode(cni.as_bytes());

    let cmd = format!("sudo bash -c \"echo {}| base64 --decode > /etc/cni/net.d/87-podman-bridge.conflist\"", cni);
    execute(conn, &cmd).await?;


    // check it's ok

    let cmd = "sudo podman run --rm -it busybox echo 1";
    execute(conn, cmd).await?;


    let cmd = "sudo mkdir -p /etc/skate";
    execute(conn, cmd).await?;

    let cmd = "sudo bash -c \"[ -f /etc/rc.local ] || touch /etc/rc.local && sudo chmod +x /etc/rc.local\"";
    execute(conn, cmd).await?;

    let cmd = "sudo bash -c \"grep -q '^/etc/skate/routes.sh' /etc/rc.local ||  echo '/etc/skate/routes.sh' >> /etc/rc.local\"";
    execute(conn, cmd).await?;

    let cmd = "sudo bash -c \"grep -q '^unqualified-search-registries' /etc/containers/registries.conf ||  echo 'unqualified-search-registries = [\\\"docker.io\\\"]' >> /etc/containers/registries.conf\"";
    execute(conn, cmd).await?;

    let (conns, errs) = cluster_connections(cluster_conf).await;
    match conns {
        Some(conns) => {
            for conn in conns.clients {
                create_replace_routes_file(&conn, cluster_conf).await?;
            }
        }
        _ => {}
    }

    Ok(())
}

async fn create_replace_routes_file(conn: &SshClient, cluster_conf: &Cluster) -> Result<(), Box<dyn Error>> {
    let other_nodes: Vec<_> = cluster_conf.nodes.iter().filter(|n| n.name != conn.node_name).collect();

    let mut route_file = "#!/bin/bash
".to_string();


    for other_node in &other_nodes {
        let ip = format!("{}:22", other_node.host).to_socket_addrs()
            .unwrap().next().unwrap().ip().to_string();
        route_file = route_file + format!("ip route add {} via {}\n", other_node.subnet_cidr, ip).as_str();
    }

    route_file = route_file + "sysctl -w net.ipv4.ip_forward=1\n";
    route_file = route_file + "sysctl -p\n";

    let route_file = general_purpose::STANDARD.encode(route_file.as_bytes());
    let cmd = format!("sudo bash -c -eu \"echo {}| base64 --decode > /etc/skate/routes.sh; chmod +x /etc/skate/routes.sh; /etc/skate/routes.sh\"", route_file);
    match execute(conn, &cmd).await {
        Ok(msg) => Ok(()),
        Err(e) => Err(e)
    }
}