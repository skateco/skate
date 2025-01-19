use crate::apply::{Apply, ApplyArgs};
use crate::config::{Cluster, Config, Node};
use crate::create::CreateDeps;
use crate::errors::SkateError;
use crate::refresh::Refresh;
use crate::resource::{ResourceType, SupportedResources};
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skate::{ConfigFileArgs, Distribution};
use crate::ssh::{SshClient, SshClients};
use crate::state::state::ClusterState;
use crate::util::{CHECKBOX_EMOJI, CROSS_EMOJI, RE_CIDR, RE_IP};
use crate::{oci, util};
use anyhow::anyhow;
use clap::Args;
use itertools::Itertools;
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::net::ToSocketAddrs;
use validator::Validate;

const COREDNS_MANIFEST: &str = include_str!("../../manifests/coredns.yaml");
const INGRESS_MANIFEST: &str = include_str!("../../manifests/ingress.yaml");

#[derive(Debug, Args, Validate)]
pub struct CreateNodeArgs {
    #[arg(long, long_help = "Name of the node.")]
    name: String,
    #[arg(long, long_help = "IP or domain name of the node from skate cli.")]
    host: String,
    #[validate(regex(path = *RE_IP, message = "peer-host must be a valid ipv4 address"))]
    #[arg(long, long_help = "IP of the node from other cluster hosts.")]
    peer_host: Option<String>,
    #[arg(long, long_help = "Ssh user for connecting")]
    user: Option<String>,
    #[arg(long, long_help = "Ssh key for connecting")]
    key: Option<String>,
    #[arg(long, long_help = "Ssh port for connecting")]
    port: Option<u16>,
    #[validate(regex(path = *RE_CIDR, message = "subnet-cidr must be a valid ipv4 cidr range"))]
    #[arg(
        long,
        long_help = "Subnet cidr for podman network (must be unique range per host)"
    )]
    subnet_cidr: String,

    #[command(flatten)]
    config: ConfigFileArgs,
}

pub async fn create_node<D: CreateDeps>(deps: &D, args: CreateNodeArgs) -> Result<(), SkateError> {
    args.validate()?;
    let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

    let mut cluster = config.active_cluster(args.config.context.clone())?.clone();

    let mut nodes_iter = cluster.nodes.clone().into_iter();

    let existing_index = nodes_iter
        .find_position(|n| n.name == args.name)
        .map(|(p, _n)| p);

    // will clobber
    // TODO - ask

    let node = Node {
        name: args.name.clone(),
        host: args.host.clone(),
        peer_host: args.peer_host.clone().unwrap_or(args.host.clone()),
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

    let conn = deps
        .get()
        .node_connect(&cluster, &node)
        .await
        .map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;
    let info = conn.get_node_system_info().await?;

    println!("{:}", &info.platform);

    conn.execute_stdout(
        "sudo apt-get update && sudo DEBIAN_FRONTEND=noninteractive apt-get -y upgrade",
        true,
        true,
    )
    .await?;

    match info.skatelet_version.as_ref() {
        None => {
            // install skatelet
            conn.install_skatelet(info.platform.clone()).await?;
        }
        Some(v) => {
            println!(
                "skatelet version {} already installed {} ",
                v, CHECKBOX_EMOJI
            )
        }
    }

    match info.podman_version.as_ref() {
        Some(version) => {
            let min_podman_ver = ">=3.0.0";
            let req = VersionReq::parse(min_podman_ver).unwrap();
            let version = Version::parse(version).unwrap();

            if !req.matches(&version) {
                return Err(anyhow!(
                    "podman version too old, must be {}, see https://podman.io/docs/installation",
                    min_podman_ver
                )
                .into());
            }
            println!(
                "podman version {} already installed {} ",
                version, CHECKBOX_EMOJI
            )
        }
        // instruct on installing newer podman version
        None => {
            let installed = match info.platform.distribution {
                Distribution::Unknown => false,
                Distribution::Debian | Distribution::Raspbian | Distribution::Ubuntu => {
                    let command =
                        "sh -c 'sudo apt-get -y update && sudo apt-get install -y podman'";
                    println!("installing podman with command {}", command);
                    let result = conn.execute(command).await;
                    match result {
                        Ok(_) => {
                            println!("podman installed successfully {} ", CHECKBOX_EMOJI);
                            true
                        }
                        Err(e) => {
                            println!("failed to install podman {} :\n{}", CROSS_EMOJI, e);
                            false
                        }
                    }
                }
            };
            if !installed {
                return Err(anyhow!(
                    "podman not installed, see https://podman.io/docs/installation"
                )
                .into());
            }
        }
    }

    // seems to be missing when using kube play
    let cmd =
        "sudo podman image exists k8s.gcr.io/pause:3.5 || sudo podman pull  k8s.gcr.io/pause:3.5";
    let _ = conn.execute_stdout(cmd, true, true).await;

    let (all_conns, _) = deps.get().cluster_connect(&cluster).await;
    let all_conns = &all_conns.unwrap_or(SshClients { clients: vec![] });

    let skate_dirs = [
        "/var/lib/skate/ingress",
        "/var/lib/skate/ingress/letsencrypt_storage",
        "/var/lib/skate/dns",
        "/var/lib/skate/keepalived",
        "/etc/skate",
    ];

    conn.execute_stdout(
        &format!("sudo mkdir -p {}", skate_dirs.join(" ")),
        true,
        true,
    )
    .await?;

    // copy rsyslog config
    conn.execute_stdout(
        &util::transfer_file_cmd(
            include_str!("../resources/10-skate.conf"),
            "/etc/rsyslog.d/10-skate.conf",
        ),
        true,
        true,
    )
    .await?;
    conn.execute_stdout(
        "sudo chown syslog:adm /etc/rsyslog.d/10-skate.conf",
        true,
        true,
    )
    .await?;
    conn.execute_stdout(
        "sudo touch /var/log/skate.log && sudo chown syslog:adm /var/log/skate.log",
        true,
        true,
    )
    .await?;
    // restart rsyslog
    conn.execute_stdout("sudo systemctl restart rsyslog", true, true)
        .await?;

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
async fn propagate_static_resources(
    _conf: &Config,
    all_conns: &SshClients,
    node: &Node,
    state: &ClusterState,
) -> Result<(), Box<dyn Error>> {
    let catalogue = state.catalogue(
        None,
        &[
            ResourceType::Ingress,
            ResourceType::Service,
            ResourceType::Secret,
        ],
    );

    let all_manifests: Result<Vec<_>, _> = catalogue
        .into_iter()
        .map(|item| SupportedResources::try_from(item.object))
        .collect();
    let all_manifests = all_manifests?;

    println!("propagating {} resources", all_manifests.len());

    let mut filtered_state = state.clone();
    filtered_state.nodes = vec![state
        .nodes
        .iter()
        .find(|n| n.node_name == node.name)
        .cloned()
        .unwrap()];

    let scheduler = DefaultScheduler {};

    // TODO - remove
    scheduler
        .schedule(all_conns, &mut filtered_state, all_manifests, false)
        .await?;

    Ok(())
}

pub async fn install_cluster_manifests<D: CreateDeps>(
    deps: &D,
    args: &ConfigFileArgs,
    config: &Cluster,
) -> Result<(), Box<dyn Error>> {
    println!("applying cluster manifests");
    // COREDNS
    // coredns listens on port 53 and 5533
    // port 53 serves .cluster.skate by forwarding to all coredns instances on port 5553
    // uses fanout plugin

    // replace forward list in coredns config with that of other hosts
    let fanout_list = config
        .nodes
        .iter()
        .map(|n| n.peer_host.clone() + ":5553")
        .join(" ");

    let coredns_yaml = COREDNS_MANIFEST.replace("%%fanout_list%%", &fanout_list);

    let coredns_yaml_path = "/tmp/skate-coredns.yaml".to_string();
    let mut file = File::create(&coredns_yaml_path)?;
    file.write_all(coredns_yaml.as_bytes())?;

    Apply::<D>::apply(
        deps,
        ApplyArgs {
            filename: vec![coredns_yaml_path],
            grace_period: 0,
            config: args.clone(),
            dry_run: false,
        },
    )
    .await?;

    // nginx ingress

    let nginx_yaml_path = "/tmp/skate-nginx-ingress.yaml".to_string();
    let mut file = File::create(&nginx_yaml_path)?;
    file.write_all(INGRESS_MANIFEST.as_bytes())?;

    Apply::<D>::apply(
        deps,
        ApplyArgs {
            filename: vec![nginx_yaml_path],
            grace_period: 0,
            config: args.clone(),
            dry_run: false,
        },
    )
    .await?;

    Ok(())
}

// TODO don't run things unless they need to be
async fn setup_networking(
    conn: &Box<dyn SshClient>,
    all_conns: &SshClients,
    cluster_conf: &Cluster,
    node: &Node,
) -> Result<(), Box<dyn Error>> {
    let network_backend = "netavark";

    conn.execute_stdout("sudo apt-get install -y keepalived", true, true)
        .await?;
    conn.execute_stdout(
        &util::transfer_file_cmd(
            include_str!("../resources/keepalived.conf"),
            "/etc/keepalived/keepalived.conf",
        ),
        true,
        true,
    )
    .await?;
    conn.execute_stdout("sudo systemctl enable keepalived", true, true)
        .await?;
    conn.execute_stdout("sudo systemctl start keepalived", true, true)
        .await?;

    if conn
        .execute_stdout("test -f /etc/containers/containers.conf", true, true)
        .await
        .is_err()
    {
        let cmd = "sudo cp /usr/share/containers/containers.conf /etc/containers/containers.conf";
        conn.execute_stdout(cmd, true, true).await?;
    } else {
        println!("containers.conf already setup {} ", CHECKBOX_EMOJI);
    }

    let cmd = format!("sudo sed -i 's&#default_subnet[ =].*&default_subnet = \"{}\"&' /etc/containers/containers.conf", node.subnet_cidr);
    conn.execute_stdout(&cmd, true, true).await?;

    let cmd = format!("sudo sed -i 's&#network_backend[ =].*&network_backend = \"{}\"&' /etc/containers/containers.conf", network_backend);
    conn.execute_stdout(&cmd, true, true).await?;

    let current_backend = conn
        .execute_noisy("sudo podman info |grep networkBackend: | awk '{print $2}'")
        .await?;
    if current_backend.trim() != network_backend {
        // Since we've changed the network backend we need to reset
        conn.execute_stdout("sudo podman system reset -f", true, true)
            .await?;
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

    let cmd = "sudo podman run --rm busybox echo '1'";
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
        conn.execute_stdout("sudo systemctl stop apparmor.service", true, true)
            .await?;
        conn.execute_stdout("sudo systemctl disable apparmor.service --now", true, true)
            .await?;
    }
    let cmd = "sudo aa-teardown";
    _ = conn.execute_stdout(cmd, true, true).await;
    let cmd = "sudo apt purge -y apparmor";
    _ = conn.execute_stdout(cmd, true, true).await;

    // disable dns services if exists
    for dns_service in ["dnsmasq", "systemd-resolved"] {
        let _ = conn.execute_stdout(&format!("sudo bash -c 'systemctl disable {dns_service}; sudo systemctl stop {dns_service}'"), true, true).await;
    }

    // changed /etc/resolv.conf to be 127.0.0.1
    // neeed to use a symlink so that it's respected and not overridden by systemd
    let cmd = "sudo bash -c 'echo 127.0.0.1 > /etc/resolv-manual.conf'";
    conn.execute_stdout(cmd, true, true).await?;
    let cmd = "sudo bash -c 'rm /etc/resolv.conf; ln -s /etc/resolv-manual.conf /etc/resolv.conf'";
    match conn.execute_stdout(cmd, true, true).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!(
                "failed to change resolv.conf, we're probably inside a container: {}",
                e
            );
        }
    }

    Ok(())
}

async fn install_oci_hooks(conn: &Box<dyn SshClient>) -> Result<(), Box<dyn Error>> {
    conn.execute_stdout(
        "sudo mkdir -p /usr/share/containers/oci/hooks.d",
        true,
        true,
    )
    .await?;

    let oci_poststart_hook = oci::HookConfig {
        version: "1.0.0".to_string(),
        hook: oci::Command {
            path: "/usr/local/bin/skatelet".to_string(),
            args: ["skatelet", "oci", "poststart"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        },
        when: oci::When {
            has_bind_mounts: None,
            annotations: Some(HashMap::from([(
                "io.container.manager".to_string(),
                "libpod".to_string(),
            )])),
            always: None,
            commands: None,
        },
        stages: vec![oci::Stage::PostStart],
    };
    // serialize to /usr/share/containers/oci/hooks.d/skatelet-poststart.json
    let serialized = serde_json::to_string(&oci_poststart_hook).unwrap();
    let path = "/usr/share/containers/oci/hooks.d/skatelet-poststart.json";
    conn.execute_stdout(
        &util::transfer_file_cmd(serialized.as_str(), path),
        true,
        true,
    )
    .await?;

    let oci_poststop = oci::HookConfig {
        version: "1.0.0".to_string(),
        hook: oci::Command {
            path: "/usr/local/bin/skatelet".to_string(),
            args: ["skatelet", "oci", "poststop"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        },
        when: oci::When {
            has_bind_mounts: None,
            annotations: Some(HashMap::from([(
                "io.container.manager".to_string(),
                "libpod".to_string(),
            )])),
            always: None,
            commands: None,
        },
        stages: vec![oci::Stage::PostStop],
    };
    let serialized = serde_json::to_string(&oci_poststop).unwrap();
    let path = "/usr/share/containers/oci/hooks.d/skatelet-poststop.json";
    conn.execute_stdout(
        &util::transfer_file_cmd(serialized.as_str(), path),
        true,
        true,
    )
    .await?;
    Ok(())
}

async fn setup_netavark(
    conn: &Box<dyn SshClient>,
    gateway: String,
    subnet_cidr: String,
) -> Result<(), Box<dyn Error>> {
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

    let netavark_config = include_str!("../resources/podman-network-netavark.json")
        .replace("%%subnet%%", &subnet_cidr)
        .replace("%%gateway%%", &gateway);

    conn.execute_stdout(
        &util::transfer_file_cmd(&netavark_config, "/etc/containers/networks/skate.json"),
        true,
        true,
    )
    .await?;
    Ok(())
}

async fn create_replace_routes_file(
    conn: &Box<dyn SshClient>,
    cluster_conf: &Cluster,
) -> Result<(), Box<dyn Error>> {
    let cmd = "sudo mkdir -p /etc/skate";
    conn.execute_stdout(cmd, true, true).await?;

    let other_nodes: Vec<_> = cluster_conf
        .nodes
        .iter()
        .filter(|n| n.name != conn.node_name())
        .collect();

    let mut route_file = "#!/bin/bash
"
    .to_string();

    for other_node in &other_nodes {
        let ip = format!("{}:22", other_node.peer_host)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap()
            .ip()
            .to_string();
        route_file += format!("ip route add {} via {}\n", other_node.subnet_cidr, ip).as_str();
    }

    // load kernel modules
    route_file +=
        "modprobe -- ip_vs\nmodprobe -- ip_vs_rr\nmodprobe -- ip_vs_wrr\nmodprobe -- ip_vs_sh\n";

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

    conn.execute_stdout(
        &util::transfer_file_cmd(&route_file, "/etc/skate/routes.sh"),
        true,
        true,
    )
    .await?;
    conn.execute_stdout(
        "sudo chmod +x /etc/skate/routes.sh; sudo /etc/skate/routes.sh",
        true,
        true,
    )
    .await?;

    // Create systemd unit file to call the skate routes file on startup after internet
    // TODO - only add if different
    let path = "/etc/systemd/system/skate-routes.service";
    let unit_file = include_str!("../resources/skate-routes.service");

    conn.execute_stdout(&util::transfer_file_cmd(unit_file, path), true, true)
        .await?;

    conn.execute_stdout("sudo systemctl daemon-reload", true, true)
        .await?;
    conn.execute_stdout("sudo systemctl enable skate-routes.service", true, true)
        .await?;
    conn.execute_stdout("sudo systemctl start skate-routes.service", true, true)
        .await?;

    Ok(())
}
