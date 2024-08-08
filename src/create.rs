use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use anyhow::anyhow;
use base64::Engine;
use base64::engine::general_purpose;
use clap::{Args, Subcommand};
use itertools::Itertools;
use semver::{Version, VersionReq};

use crate::apply::{apply, ApplyArgs};
use crate::config::{Cluster, Config, Node};
use crate::refresh::refreshed_state;
use crate::skate::{ConfigFileArgs, Distribution};

use crate::ssh::{cluster_connections, node_connection, SshClient, SshClients};

use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};

const COREDNS_MANIFEST: &str = include_str!("../manifests/coredns.yaml");
const INGRESS_MANIFEST: &str = include_str!("../manifests/ingress.yaml");

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
                Err(anyhow!("--context is required unless there is already a current context"))
            }
            Some(ref context) => Ok(context)
        }
        Some(ref context) => Ok(context)
    }.map_err(Into::<Box<dyn Error>>::into)?;

    let (cluster_index, cluster) = config.clusters.iter().find_position(|c| c.name == context.clone()).ok_or(anyhow!("no cluster by name of {}", context))?;
    let mut cluster = (*cluster).clone();

    let mut nodes_iter = cluster.nodes.clone().into_iter();

    let existing_index = nodes_iter.find_position(|n| n.name == args.name || n.host == args.host).map(|(p, _n)| p);

    // will clobber
    // TODO - ask

    let node = Node {
        name: args.name.clone(),
        host: args.host.clone(),
        port: args.port.clone(),
        user: args.user.clone(),
        key: args.key.clone(),
        subnet_cidr: args.subnet_cidr.clone(),

    };

    match existing_index {
        Some(idx) => {
            cluster.nodes[idx] = node.clone();
        }
        None => {
            cluster.nodes.push(node.clone());
        }
    };


    config.clusters[cluster_index] = cluster.clone();
    config.persist(Some(args.config.skateconfig.clone()))?;

    let conn = node_connection(&cluster, &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;
    let info = conn.get_node_system_info().await?;

    println!("{:}", &info.platform);

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
            let installed = match info.platform.distribution {
                Distribution::Unknown => false,
                Distribution::Debian | Distribution::Raspbian | Distribution::Ubuntu => {
                    let command = "sh -c 'sudo apt-get -y update && sudo apt-get install -y podman containernetworking-plugins'";
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
            };
            if !installed {
                return Err(anyhow!("podman not installed, see https://podman.io/docs/installation").into());
            }
        }
    }

    // seems to be missing when using kube play
    let cmd = "sudo podman image exists k8s.gcr.io/pause:3.5 || sudo podman pull  k8s.gcr.io/pause:3.5";
    conn.execute(cmd).await?;

    let (all_conns, _) = cluster_connections(&cluster).await;
    let all_conns = &all_conns.unwrap_or(SshClients { clients: vec!() });


    _ = conn.execute("sudo mkdir -p /var/lib/skate/ingress /var/lib/skate/ingress/letsencrypt_storage").await?;
    // _ = conn.execute("sudo podman rm -fa").await;

    setup_networking(&conn, &all_conns, &cluster, &node).await?;

    config.persist(Some(args.config.skateconfig.clone()))?;

    // Refresh state so that we can apply coredns later
    let state = refreshed_state(&cluster.name, &all_conns, &config).await?;
    state.persist()?;

    install_manifests(&args, &cluster, &node).await?;

    Ok(())
}

async fn install_manifests(args: &CreateNodeArgs, config: &Cluster, node: &Node) -> Result<(), Box<dyn Error>> {

    // COREDNS
    // coredns listens on port 53 and 5533
    // port 53 serves .cluster.skate by forwarding to all coredns instances on port 5553
    // uses fanout plugin

    // replace forward list in coredns config with that of other hosts
    let fanout_list = config.nodes.iter().map(|n| n.host.clone() + ":5553").join(" ");

    let coredns_yaml = COREDNS_MANIFEST.replace("%%fanout_list%%", &fanout_list);

    let coredns_yaml_path = format!("/tmp/skate-coredns-{}.yaml", node.name);
    let mut file = File::create(&coredns_yaml_path)?;
    file.write_all(coredns_yaml.as_bytes())?;


    apply(ApplyArgs {
        filename: vec![coredns_yaml_path],
        grace_period: 0,
        config: args.config.clone(),
    }).await?;

    // nginx ingress

    let nginx_yaml_path = format!("/tmp/skate-nginx-ingress-{}.yaml", node.name);
    let mut file = File::create(&nginx_yaml_path)?;
    file.write_all(INGRESS_MANIFEST.as_bytes())?;


    apply(ApplyArgs {
        filename: vec![nginx_yaml_path],
        grace_period: 0,
        config: args.config.clone(),
    }).await?;

    Ok(())
}

async fn setup_networking(conn: &SshClient, all_conns: &SshClients, cluster_conf: &Cluster, node: &Node) -> Result<(), Box<dyn Error>> {
    let cmd = "sqlite3 -version || sudo apt-get install -y sqlite3";
    conn.execute(cmd).await?;

    let cmd = "sudo cp /usr/share/containers/containers.conf /etc/containers/containers.conf";
    conn.execute(cmd).await?;

    let cmd = format!("sudo sed -i 's&#default_subnet[ =].*&default_subnet = \"{}\"&' /etc/containers/containers.conf", node.subnet_cidr);
    conn.execute(&cmd).await?;
    let cmd = "sudo sed -i 's&#network_backend[ =].*&network_backend = \"cni\"&' /etc/containers/containers.conf";
    conn.execute(&cmd).await?;

    let cmd = "sudo ip link del cni-podman0|| exit 0";
    conn.execute(&cmd).await?;

    let gateway = node.subnet_cidr.split(".").take(3).join(".") + ".1";
    // only allocate from ip 10 onwards, reserves 1-9 for other stuff
    let cni = include_str!("./resources/podman-network.json").replace("%%subnet%%", &node.subnet_cidr)
        .replace("%%gateway%%", &gateway);

    let cni = general_purpose::STANDARD.encode(cni.as_bytes());

    let cmd = format!("sudo bash -c \"echo {}| base64 --decode > /etc/cni/net.d/87-podman-bridge.conflist\"", cni);
    conn.execute(&cmd).await?;

    let cni_script = general_purpose::STANDARD.encode("#!/bin/sh
    exec /usr/local/bin/skatelet cni < /dev/stdin
    ");

    let cmd = format!("sudo bash -c 'echo {} | base64 --decode > /usr/lib/cni/skatelet; chmod +x /usr/lib/cni/skatelet'", cni_script);
    conn.execute(&cmd).await?;
    // check it's ok

    let cmd = "sudo podman run --rm -it busybox echo 1";
    conn.execute(cmd).await?;


    let cmd = "sudo mkdir -p /etc/skate";
    conn.execute(cmd).await?;


    let cmd = "sudo bash -c \"grep -q '^unqualified-search-registries' /etc/containers/registries.conf ||  echo 'unqualified-search-registries = [\\\"docker.io\\\"]' >> /etc/containers/registries.conf\"";
    conn.execute(cmd).await?;


    for conn in &all_conns.clients {
        create_replace_routes_file(conn, cluster_conf).await?;
    }

    let cmd = "sudo podman image exists ghcr.io/skateco/coredns || sudo podman pull ghcr.io/skateco/coredns";
    conn.execute(cmd).await?;


    // In ubuntu 24.04 there's an issue with apparmor and podman
    // https://bugs.launchpad.net/ubuntu/+source/libpod/+bug/2040483

    let cmd = "sudo systemctl list-unit-files apparmor.service";
    let apparmor_unit_exists = conn.execute(cmd).await;

    if apparmor_unit_exists.is_ok() {
        let cmd = "sudo systemctl disable apparmor.service --now";
        conn.execute(cmd).await?;
    }
    let cmd = "sudo aa-teardown";
    _ = conn.execute(cmd).await;
    let cmd = "sudo apt purge -y apparmor";
    _ = conn.execute(cmd).await;


    // disable systemd-resolved if exists
    let cmd = "sudo bash -c 'systemctl disable systemd-resolved; sudo systemctl stop systemd-resolved'";
    conn.execute(cmd).await?;

    // changed /etc/resolv.conf to be 127.0.0.1
    // neeed to use a symlink so that it's respected and not overridden by systemd
    let cmd = "sudo bash -c 'echo 127.0.0.1 > /etc/resolv-manual.conf'";
    conn.execute(cmd).await?;
    let cmd = "sudo bash -c 'rm /etc/resolv.conf && ln -s /etc/resolv-manual.conf /etc/resolv.conf'";
    conn.execute(cmd).await?;

    Ok(())
}

async fn create_replace_routes_file(conn: &SshClient, cluster_conf: &Cluster) -> Result<(), Box<dyn Error>> {
    let cmd = "sudo mkdir -p /etc/skate";
    conn.execute(cmd).await?;

    let other_nodes: Vec<_> = cluster_conf.nodes.iter().filter(|n| n.name != conn.node_name).collect();

    let mut route_file = "#!/bin/bash
".to_string();

    for other_node in &other_nodes {
        let ip = format!("{}:22", other_node.host).to_socket_addrs()
            .unwrap().next().unwrap().ip().to_string();
        route_file = route_file + format!("ip route add {} via {}\n", other_node.subnet_cidr, ip).as_str();
    }

    route_file = route_file + "sysctl -w net.ipv4.ip_forward=1\n";
    route_file = route_file + "sysctl fs.inotify.max_user_instances=1280\n";
    route_file = route_file + "sysctl fs.inotify.max_user_watches=655360\n";
    route_file = route_file + "sysctl -p\n";


    let route_file = general_purpose::STANDARD.encode(route_file.as_bytes());
    let cmd = format!("sudo bash -c -eu \"echo {}| base64 --decode > /etc/skate/routes.sh; chmod +x /etc/skate/routes.sh; /etc/skate/routes.sh\"", route_file);
    conn.execute(&cmd).await?;


    // Create systemd unit file to call the skate routes file on startup after internet
    let path = "/etc/systemd/system/skate-routes.service";
    let unit_file = include_str!("./resources/skate-routes.service");
    let unit_file = general_purpose::STANDARD.encode(unit_file.as_bytes());

    let cmd = format!("sudo bash -c -eu \"echo {}| base64 --decode > {}\"", unit_file, path);
    conn.execute(&cmd).await?;

    conn.execute("sudo systemctl daemon-reload").await?;
    conn.execute("sudo systemctl enable skate-routes.service").await?;
    _ = conn.execute("sudo systemctl start skate-routes.service").await?;

    Ok(())
}