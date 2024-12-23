use std::error::Error;
use anyhow::anyhow;
use semver::{Version, VersionReq};
use std::fs::File;
use base64::engine::general_purpose;
use std::collections::HashMap;
use clap::Args;
use itertools::Itertools;
use std::io::Write;
use base64::Engine;
use std::net::ToSocketAddrs;
use crate::apply::{Apply, ApplyArgs};
use crate::config::{Cluster, Config, Node};
use crate::create::CreateDeps;
use crate::deps::SshManager;
use crate::oci;
use crate::refresh::Refresh;
use crate::resource::{ResourceType, SupportedResources};
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skate::{ConfigFileArgs, Distribution};
use crate::ssh::{SshClient, SshClients};
use crate::state::state::ClusterState;
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI};

const COREDNS_MANIFEST: &str = include_str!("../../manifests/coredns.yaml");
const INGRESS_MANIFEST: &str = include_str!("../../manifests/ingress.yaml");

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


pub async fn create_node<D: CreateDeps>(deps: &D, args: CreateNodeArgs) -> Result<(), Box<dyn Error>> {
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let mut cluster = config.active_cluster(args.config.context.clone())?.clone();

    let mut nodes_iter = cluster.nodes.clone().into_iter();

    let existing_index = nodes_iter.find_position(|n| n.name == args.name || n.host == args.host).map(|(p, _n)| p);

    // will clobber
    // TODO - ask

    let node = Node {
        name: args.name.clone(),
        host: args.host.clone(),
        port: args.port,
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

    config.replace_cluster(&cluster)?;


    config.persist(Some(args.config.skateconfig.clone()))?;
    

    let conn = deps.get().node_connect(&cluster, &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;
    let info = conn.get_node_system_info().await?;

    println!("{:}", &info.platform);

    conn.execute_stdout("sudo apt-get update && sudo DEBIAN_FRONTEND=noninteractive apt-get -y upgrade", true, true).await?;

    match info.skatelet_version.as_ref() {
        None => {
            // install skatelet
            conn.install_skatelet(info.platform.clone()).await?;
        }
        Some(v) => {
            println!("skatelet version {} already installed {} ", v, CHECKBOX_EMOJI)
        }
    }

    match info.podman_version.as_ref() {
        Some(version) => {
            let min_podman_ver = ">=3.0.0";
            let req = VersionReq::parse(min_podman_ver).unwrap();
            let version = Version::parse(version).unwrap();

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
                    let command = "sh -c 'sudo apt-get -y update && sudo apt-get install -y podman'";
                    println!("installing podman with command {}", command);
                    let result = conn.execute(command).await;
                    match result {
                        Ok(output) => {
                            println!("podman installed successfully {} ", CHECKBOX_EMOJI);
                            true
                        },
                        Err(e) => {
                            println!("failed to install podman {} :\n{}", CROSS_EMOJI, e);
                            false
                        }
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
    let _ = conn.execute_stdout(cmd, true, true).await;

    let (all_conns, _) = deps.get().cluster_connect(&cluster).await;
    let all_conns = &all_conns.unwrap_or(SshClients { clients: vec!() });

    let skate_dirs = [
        "ingress",
        "ingress/letsencrypt_storage",
        "dns",
        "keepalived"].map(|s| format!("/var/lib/skate/{}", s));

    conn.execute_stdout(&format!("sudo mkdir -p {}", skate_dirs.join(" ")), true, true).await?;
    // _ = conn.execute("sudo podman rm -fa").await;

    setup_networking(&conn, all_conns, &cluster, &node).await?;

    config.persist(Some(args.config.skateconfig.clone()))?;

    // Refresh state so that we can apply coredns later
    let state = Refresh::<D>::refreshed_state(&cluster.name, all_conns, &config).await?;

    install_cluster_manifests(deps, &args.config, &cluster).await?;

    propagate_static_resources(&config, all_conns, &node, &state).await?;

    Ok(())
}

// propagate existing resources to new node, such as secrets, ingress, services
// for now just takes them from the first node
// TODO - do some kind of lookup and merge
// could be to take only resources that are the same on all nodes, log others
async fn propagate_static_resources(_conf: &Config, all_conns: &SshClients, node: &Node, state: &ClusterState) -> Result<(), Box<dyn Error>> {

    
    let catalogue = state.catalogue(None, &[ResourceType::Ingress, ResourceType::Service, ResourceType::Secret]);



    let all_manifests: Result<Vec<_>, _> = catalogue.into_iter().map(|item| SupportedResources::try_from(item.object)).collect();
    let all_manifests = all_manifests?;
    
    println!("propagating {} resources", all_manifests.len());


    let mut filtered_state = state.clone();
    filtered_state.nodes = vec!(state.nodes.iter().find(|n| n.node_name == node.name).cloned().unwrap());


    let scheduler = DefaultScheduler {};

    // TODO - remove
    scheduler.schedule(all_conns, &mut filtered_state, all_manifests, false).await?;

    Ok(())
}

pub async fn install_cluster_manifests<D: CreateDeps>(deps: &D, args: &ConfigFileArgs, config: &Cluster) -> Result<(), Box<dyn Error>> {
    println!("applying cluster manifests");
    // COREDNS
    // coredns listens on port 53 and 5533
    // port 53 serves .cluster.skate by forwarding to all coredns instances on port 5553
    // uses fanout plugin

    // replace forward list in coredns config with that of other hosts
    let fanout_list = config.nodes.iter().map(|n| n.host.clone() + ":5553").join(" ");

    let coredns_yaml = COREDNS_MANIFEST.replace("%%fanout_list%%", &fanout_list);

    let coredns_yaml_path = "/tmp/skate-coredns.yaml".to_string();
    let mut file = File::create(&coredns_yaml_path)?;
    file.write_all(coredns_yaml.as_bytes())?;


    Apply::<D>::apply(deps, ApplyArgs {
        filename: vec![coredns_yaml_path],
        grace_period: 0,
        config: args.clone(),
        dry_run: false,
    }).await?;

    // nginx ingress

    let nginx_yaml_path = "/tmp/skate-nginx-ingress.yaml".to_string();
    let mut file = File::create(&nginx_yaml_path)?;
    file.write_all(INGRESS_MANIFEST.as_bytes())?;


    Apply::<D>::apply(deps, ApplyArgs {
        filename: vec![nginx_yaml_path],
        grace_period: 0,
        config: args.clone(),
        dry_run: false,
    }).await?;

    Ok(())
}

// TODO don't run things unless they need to be
async fn setup_networking(conn: &Box<dyn SshClient>, all_conns: &SshClients, cluster_conf: &Cluster, node: &Node) -> Result<(), Box<dyn Error>> {
    let network_backend = "netavark";

    conn.execute_stdout("sudo apt-get install -y keepalived", true, true).await?;
    conn.execute_stdout(&format!("sudo bash -c -eu 'echo {}| base64 --decode > /etc/keepalived/keepalived.conf'", general_purpose::STANDARD.encode(include_str!("../resources/keepalived.conf"))), true, true).await?;
    conn.execute_stdout("sudo systemctl enable keepalived", true, true).await?;
    conn.execute_stdout("sudo systemctl start keepalived", true, true).await?;


    if conn.execute_stdout("test -f /etc/containers/containers.conf", true, true).await.is_err() {
        let cmd = "sudo cp /usr/share/containers/containers.conf /etc/containers/containers.conf";
        conn.execute_stdout(cmd, true, true).await?;

        let cmd = format!("sudo sed -i 's&#default_subnet[ =].*&default_subnet = \"{}\"&' /etc/containers/containers.conf", node.subnet_cidr);
        conn.execute_stdout(&cmd, true, true).await?;

        let cmd = format!("sudo sed -i 's&#network_backend[ =].*&network_backend = \"{}\"&' /etc/containers/containers.conf", network_backend);
        conn.execute_stdout(&cmd, true, true).await?;

        let current_backend = conn.execute_noisy("sudo podman info |grep networkBackend: | awk '{print $2}'").await?;
        if current_backend.trim() != network_backend {
            // Since we've changed the network backend we need to reset
            conn.execute_stdout("sudo podman system reset -f", true, true).await?;
        }
    } else {
        println!("containers.conf already setup {} ", CHECKBOX_EMOJI);
    }

    let gateway = node.subnet_cidr.split(".").take(3).join(".") + ".1";
    // only allocate from ip 10 onwards, reserves 1-9 for other stuff

    match network_backend {
        "cni" => {
            return Err(anyhow!("cni is deprecated, use netavark").into());
        }
        "netavark" => {
            setup_netavark(conn, gateway.clone(), node.subnet_cidr.clone()).await?;
        }
        _ => {
            return Err(anyhow!("unknown network backend {}", network_backend).into());
        }
    }

    install_oci_hooks(conn).await?;

    let cmd = "sudo podman run --rm busybox echo 1";
    conn.execute_stdout(cmd, true, true).await?;


    let cmd = "sudo mkdir -p /etc/skate";
    conn.execute_stdout(cmd, true, true).await?;


    let cmd = "sudo bash -c \"grep -q '^unqualified-search-registries' /etc/containers/registries.conf ||  echo 'unqualified-search-registries = [\\\"docker.io\\\"]' >> /etc/containers/registries.conf\"";
    conn.execute_stdout(cmd, true, true).await?;


    for conn in &all_conns.clients {
        create_replace_routes_file(conn, cluster_conf).await?;
    }

    let cmd = "sudo podman image exists ghcr.io/skateco/coredns || sudo podman pull ghcr.io/skateco/coredns";
    conn.execute_stdout(cmd, true, true).await?;


    // In ubuntu 24.04 there's an issue with apparmor and podman
    // https://bugs.launchpad.net/ubuntu/+source/libpod/+bug/2040483

    let cmd = "sudo systemctl list-unit-files apparmor.service";
    let apparmor_unit_exists = conn.execute_stdout(cmd, true, true).await;

    if apparmor_unit_exists.is_ok() {
        conn.execute_stdout("sudo systemctl stop apparmor.service", true, true).await?;
        conn.execute_stdout("sudo systemctl disable apparmor.service --now", true, true).await?;
    }
    let cmd = "sudo aa-teardown";
    _ = conn.execute_stdout(cmd, true, true).await;
    let cmd = "sudo apt purge -y apparmor";
    _ = conn.execute_stdout(cmd, true, true).await;


    // disable systemd-resolved if exists
    let cmd = "sudo bash -c 'systemctl disable systemd-resolved; sudo systemctl stop systemd-resolved'";
    conn.execute_stdout(cmd, true, true).await?;

    // changed /etc/resolv.conf to be 127.0.0.1
    // neeed to use a symlink so that it's respected and not overridden by systemd
    let cmd = "sudo bash -c 'echo 127.0.0.1 > /etc/resolv-manual.conf'";
    conn.execute_stdout(cmd, true, true).await?;
    let cmd = "sudo bash -c 'rm /etc/resolv.conf && ln -s /etc/resolv-manual.conf /etc/resolv.conf'";
    conn.execute_stdout(cmd, true, true).await?;

    Ok(())
}

async fn install_oci_hooks(conn: &Box<dyn SshClient>) -> Result<(), Box<dyn Error>> {
    conn.execute_stdout("sudo mkdir -p /usr/share/containers/oci/hooks.d", true, true).await?;

    let oci_poststart_hook = oci::HookConfig {
        version: "1.0.0".to_string(),
        hook: oci::Command {
            path: "/usr/local/bin/skatelet".to_string(),
            args: ["skatelet", "oci", "poststart"].into_iter().map(|s| s.to_string()).collect(),
        },
        when: oci::When {
            has_bind_mounts: None,
            annotations: Some(HashMap::from([("io.container.manager".to_string(), "libpod".to_string())])),
            always: None,
            commands: None,
        },
        stages: vec![oci::Stage::PostStart],
    };
    // serialize to /usr/share/containers/oci/hooks.d/skatelet-poststart.json
    let serialized = serde_json::to_string(&oci_poststart_hook).unwrap();

    let path = "/usr/share/containers/oci/hooks.d/skatelet-poststart.json";
    let cmd = format!("sudo bash -c -eu 'echo {}| base64 --decode > {}'", general_purpose::STANDARD.encode(serialized.as_bytes()), path);
    conn.execute_stdout(&cmd, true, true).await?;


    let oci_poststop = oci::HookConfig {
        version: "1.0.0".to_string(),
        hook: oci::Command {
            path: "/usr/local/bin/skatelet".to_string(),
            args: ["skatelet", "oci", "poststop"].into_iter().map(|s| s.to_string()).collect(),
        },
        when: oci::When {
            has_bind_mounts: None,
            annotations: Some(HashMap::from([("io.container.manager".to_string(), "libpod".to_string())])),
            always: None,
            commands: None,
        },
        stages: vec![oci::Stage::PostStop],
    };
    let serialized = serde_json::to_string(&oci_poststop).unwrap();
    let path = "/usr/share/containers/oci/hooks.d/skatelet-poststop.json";
    let cmd = format!("sudo bash -c -eu 'echo {}| base64 --decode > {}'", general_purpose::STANDARD.encode(serialized.as_bytes()), path);
    conn.execute_stdout(&cmd, true, true).await?;
    Ok(())
}

async fn setup_netavark(conn: &Box<dyn SshClient>, gateway: String, subnet_cidr: String) -> Result<(), Box<dyn Error>> {
    println!("installing netavark");
    // // The netavark plugin isn't actually used right now but we'll put it there just in case.
    // // We'll use an oci hook instead.
    // let netavark_script = general_purpose::STANDARD.encode("#!/bin/sh
    // exec /usr/local/bin/skatelet-netavark < /dev/stdin
    // ");
    //
    // conn.execute("sudo mkdir -p /usr/lib/netavark").await?;
    //
    // let cmd = format!("sudo bash -c 'echo {} | base64 --decode > /usr/lib/netavark/skatelet; chmod +x /usr/lib/netavark/skatelet'", netavark_script);
    // conn.execute(&cmd).await?;
    // // check it's ok

    let netavark_config = include_str!("../resources/podman-network-netavark.json").replace("%%subnet%%", &subnet_cidr)
        .replace("%%gateway%%", &gateway);

    let netvark_config = general_purpose::STANDARD.encode(netavark_config.as_bytes());

    let cmd = format!("sudo bash -c \"echo {}| base64 --decode > /etc/containers/networks/skate.json\"", netvark_config);
    conn.execute_stdout(&cmd, true, true).await?;
    Ok(())
}

async fn create_replace_routes_file(conn: &Box<dyn SshClient>, cluster_conf: &Cluster) -> Result<(), Box<dyn Error>> {
    let cmd = "sudo mkdir -p /etc/skate";
    conn.execute_stdout(cmd, true, true).await?;

    let other_nodes: Vec<_> = cluster_conf.nodes.iter().filter(|n| n.name != conn.node_name()).collect();

    let mut route_file = "#!/bin/bash
".to_string();

    for other_node in &other_nodes {
        let ip = format!("{}:22", other_node.host).to_socket_addrs()
            .unwrap().next().unwrap().ip().to_string();
        route_file += format!("ip route add {} via {}\n", other_node.subnet_cidr, ip).as_str();
    }

    // load kernel modules
    route_file += "modprobe -- ip_vs\nmodprobe -- ip_vs_rr\nmodprobe -- ip_vs_wrr\nmodprobe -- ip_vs_sh\n";

    route_file += "sysctl -w net.ipv4.ip_forward=1\n";
    route_file += "sysctl fs.inotify.max_user_instances=1280\n";
    route_file += "sysctl fs.inotify.max_user_watches=655360\n";

    // Virtual Server stuff
    // taken from https://github.com/kubernetes/kubernetes/blob/master/pkg/proxy/ipvs/proxier.go#L295
    route_file += "sysctl -w net.ipv4.vs.conntrack=1\n";
    // since we're using conntrac we need to increase the max so we dont exhaust it
    route_file += "sysctl net.nf_conntrack_max=512000\n";
    route_file += "sysctl -w net.ipv4.vs.conn_reuse_mode=0\n";
    route_file += "sysctl -w net.ipv4.vs.expire_nodest_conn=1\n";
    route_file += "sysctl -w net.ipv4.vs.expire_quiescent_template=1\n";
    // configurable in kube-proxy
    // route_file = route_file + "sysctl -w net.ipv4.conf.all.arp_ignore=1\n";
    // route_file = route_file + "sysctl -w net.ipv4.conf.all.arp_announce=2\n";

    route_file += "sysctl -p\n";


    let route_file = general_purpose::STANDARD.encode(route_file.as_bytes());
    let cmd = format!("sudo bash -c -eu 'echo {}| base64 --decode > /etc/skate/routes.sh; chmod +x /etc/skate/routes.sh; /etc/skate/routes.sh'", route_file);
    conn.execute_stdout(&cmd, true, true).await?;


    // Create systemd unit file to call the skate routes file on startup after internet
    // TODO - only add if different
    let path = "/etc/systemd/system/skate-routes.service";
    let unit_file = include_str!("../resources/skate-routes.service");
    let unit_file = general_purpose::STANDARD.encode(unit_file.as_bytes());

    let cmd = format!("sudo bash -c -eu 'echo {}| base64 --decode > {}'", unit_file, path);
    conn.execute_stdout(&cmd, true, true).await?;

    conn.execute_stdout("sudo systemctl daemon-reload", true, true).await?;
    conn.execute_stdout("sudo systemctl enable skate-routes.service", true, true).await?;
    conn.execute_stdout("sudo systemctl start skate-routes.service", true, true).await?;

    Ok(())
}