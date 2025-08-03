use crate::apply::{Apply, ApplyArgs};
use crate::config::{Cluster, Config, Node};
use crate::create::CreateDeps;
use crate::errors::SkateError;
use crate::refresh::Refresh;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skate::{ConfigFileArgs, Distribution};
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::VAR_PATH;
use crate::ssh::{SshClient, SshClients};
use crate::state::state::{ClusterState, NodeState};
use crate::supported_resources::SupportedResources;
use crate::util::{
    split_container_image, transfer_file_cmd, ImageTagFormat, CHECKBOX_EMOJI, CROSS_EMOJI, RE_CIDR,
    RE_HOSTNAME, RE_HOST_SEGMENT, RE_IP,
};
use crate::{oci, util};
use anyhow::anyhow;
use clap::Args;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::DaemonSet;
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
    #[validate(
        regex(path = *RE_HOST_SEGMENT, message = "name can only contain a-z, 0-9, _ or -"),
        length(min = 1, max = 128)
    )]
    #[arg(long, long_help = "Name of the node.")]
    name: String,
    #[validate(length(max = 253))]
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

trait CommandVariant {
    fn system_update(&self) -> String;
    fn install_podman(&self) -> String;
    fn install_keepalived(&self) -> String;
    fn remove_kernel_security(&self) -> String;

    fn configure_etc_containers_registries(&self) -> String;

    fn configure_firewall(&self) -> String;
}

struct UbuntuProvisioner {}
struct FedoraProvisioner {}

struct FedoraCoreosProvisioner {}

impl CommandVariant for UbuntuProvisioner {
    fn system_update(&self) -> String {
        "sudo apt-get update && sudo DEBIAN_FRONTEND=noninteractive apt-get -y upgrade".into()
    }

    fn install_podman(&self) -> String {
        "sudo apt-get install -y podman".into()
    }
    fn install_keepalived(&self) -> String {
        "sudo apt-get install -y keepalived".into()
    }

    fn remove_kernel_security(&self) -> String {
        "sudo aa-teardown && sudo apt purge -y apparmor".into()
    }
    fn configure_etc_containers_registries(&self) -> String {
        "".into()
    }

    fn configure_firewall(&self) -> String {
        "".into()
    }
}

impl CommandVariant for FedoraProvisioner {
    fn system_update(&self) -> String {
        "sudo dnf -y update && sudo dnf -y upgrade".into()
    }

    fn install_podman(&self) -> String {
        "sudo dnf -y install podman".into()
    }

    fn install_keepalived(&self) -> String {
        "sudo dnf -y install keepalived".into()
    }

    fn remove_kernel_security(&self) -> String {
        "sudo setenforce 0; sudo sed -i 's/^SELINUX=.*/SELINUX=permissive/' /etc/selinux/config"
            .into()
    }

    fn configure_etc_containers_registries(&self) -> String {
        r#"sudo bash -c "sed -i 's|^[\#]\?short-name-mode\s\?=.*|short-name-mode=\"permissive\"|g' /etc/containers/registries.conf""#.into()
    }

    fn configure_firewall(&self) -> String {
        "sudo systemctl stop firewalld; sudo systemctl disable firewalld".into()
    }
}

impl CommandVariant for FedoraCoreosProvisioner {
    fn system_update(&self) -> String {
        "".into()
    }

    fn install_podman(&self) -> String {
        "sudo rpm-ostree -y install podman".into()
    }

    fn install_keepalived(&self) -> String {
        "sudo rpm-ostree -y install keepalived".into()
    }

    fn remove_kernel_security(&self) -> String {
        "sudo setenforce 0; sudo sed -i 's/^SELINUX=.*/SELINUX=permissive/' /etc/selinux/config"
            .into()
    }

    fn configure_etc_containers_registries(&self) -> String {
        r#"sudo bash -c "sed -i 's|^[\#]\?short-name-mode\s\?=.*|short-name-mode=\"permissive\"|g' /etc/containers/registries.conf""#.into()
    }

    fn configure_firewall(&self) -> String {
        "".into()
    }
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
    let provisioner: Box<dyn CommandVariant> = match info.platform.distribution {
        Distribution::Debian | Distribution::Raspbian | Distribution::Ubuntu => {
            Box::new(UbuntuProvisioner {})
        }
        Distribution::Fedora => Box::new(FedoraProvisioner {}),
        Distribution::FedoraCoreOs => Box::new(FedoraCoreosProvisioner {}),
        Distribution::Unknown => {
            return Err(anyhow!("unknown distribution").into());
        }
    };

    conn.execute_stdout(&provisioner.system_update(), true, true)
        .await?;

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
            let command = provisioner.install_podman();

            let installed = {
                println!("installing podman with command {}", command);
                let result = conn.execute(&command).await;
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
            };

            if !installed {
                return Err(anyhow!(
                    "podman not installed, see https://podman.io/docs/installation"
                )
                .into());
            }
        }
    }

    // enable the restart service
    conn.execute_stdout("sudo systemctl enable podman-restart.service && sudo systemctl start podman-restart.service", true, true).await?;

    let (all_conns, _) = deps.get().cluster_connect(&cluster).await;
    let all_conns = &all_conns.unwrap_or(SshClients { clients: vec![] });

    let skate_dirs: [&str; 4] = [
        &format!("{VAR_PATH}/ingress"),
        &format!("{VAR_PATH}/ingress/letsencrypt_storage"),
        &format!("{VAR_PATH}/dns"),
        &format!("{VAR_PATH}/keepalived"),
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

    // ensure syslog user exists
    conn.execute_stdout(
        "sudo useradd syslog -g adm || echo \"syslog user already exists\"",
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

    conn.execute_stdout("sudo touch /var/log/skate.log", true, true)
        .await?;

    conn.execute_stdout("sudo chown syslog:adm /var/log/skate.log", true, true)
        .await?;
    // restart rsyslog
    conn.execute_stdout("sudo systemctl restart rsyslog", true, true)
        .await?;

    let (coredns_image, coredns_tag) = get_coredns_image()?;

    if matches!(coredns_tag, ImageTagFormat::None) {
        return Err(anyhow!("coredns image tag not found").into());
    }

    setup_networking(
        &conn,
        all_conns,
        &cluster,
        &node,
        coredns_image,
        coredns_tag,
        provisioner,
    )
    .await?;

    config.persist(Some(args.config.skateconfig.clone()))?;

    // Refresh state so that we can apply coredns later
    let state = Refresh::<D>::refreshed_state(&cluster.name, all_conns, &config).await?;

    install_cluster_manifests(deps, &args.config, &cluster).await?;

    propagate_static_resources(&config, all_conns, &node, &state).await?;

    Ok(())
}

fn get_coredns_image() -> Result<(String, ImageTagFormat), Box<dyn Error>> {
    let manifest: DaemonSet = serde_yaml::from_str(COREDNS_MANIFEST)?;

    let image = manifest
        .spec
        .and_then(|s| s.template.spec.and_then(|t| t.containers[0].image.clone()));

    if image.is_none() {
        return Err(anyhow!("failed to get coredns image").into());
    }

    let image = image.unwrap();

    Ok(split_container_image(&image))
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
        None,
        None,
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

    let scheduler = DefaultScheduler::new();

    // TODO - remove
    scheduler
        .schedule(all_conns, &mut filtered_state, all_manifests, false)
        .await?;

    Ok(())
}

fn template_coredns_manifest(config: &Cluster) -> String {
    let noop_dns_server = "127.0.0.1:6053";

    let mut gathersrv_list = vec![];
    let mut forward_list = vec![];
    let mut rewrite_list = vec![];

    let padding = " ".repeat(15); // get past the yaml indentation

    // the gathersrv stanza needs a list of upstreams to forward to
    config.nodes.iter().enumerate().for_each(|(i, n)| {
        let node_name = &n.name;
        let domain = format!("pod.n-{node_name}.skate.",);
        let peer_host = &n.peer_host;

        gathersrv_list.push(format!("{padding}{domain} {i}"));

        forward_list.push(format!(
            r#"{padding}forward {domain} {peer_host}:5553 {noop_dns_server} {{
{padding}    policy sequential
{padding}    health_check 0.5s
{padding}}}"#,
        ));
        rewrite_list.push(format!(
            "{padding}rewrite name suffix .n-{node_name}.skate. .cluster.skate."
        ))
    });

    let coredns_yaml = COREDNS_MANIFEST
        .replace("%%rewrite_list%%", &rewrite_list.join("\n"))
        .replace("%%forward_list%%", &forward_list.join("\n"))
        .replace("%%gathersrv_list%%", &gathersrv_list.join("\n"));

    coredns_yaml
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
    // uses gathersrv plugin
    let coredns_yaml = template_coredns_manifest(config);

    let coredns_yaml_path = format!("/tmp/skate-coredns.yaml");
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
    coredns_image: String,
    coredns_tag: ImageTagFormat,
    provisioner: Box<dyn CommandVariant>,
) -> Result<(), Box<dyn Error>> {
    let network_backend = "netavark";

    conn.execute_stdout(&provisioner.install_keepalived(), true, true)
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

    let cmd = provisioner.configure_etc_containers_registries();
    if !cmd.is_empty() {
        conn.execute_stdout(&cmd, true, true).await?
    }

    for conn in &all_conns.clients {
        // sync peers
        sync_peers(&conn, cluster_conf).await?
    }

    let coredns_tag = coredns_tag.to_suffix();

    let cmd = format!("sudo podman image exists {coredns_image}{coredns_tag} || sudo podman pull {coredns_image}{coredns_tag}");
    conn.execute_stdout(&cmd, true, true).await?;

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

    let cmd = provisioner.remove_kernel_security();
    _ = conn.execute_stdout(&cmd, true, true).await;
    let cmd = provisioner.configure_firewall();
    _ = conn.execute_stdout(&cmd, true, true).await;

    // create dropin dir for resolved
    conn.execute_stdout(
        "sudo mkdir -p  /etc/systemd/resolved.conf.d/ /etc/dnsmasq.d/",
        true,
        true,
    )
    .await?;

    conn.execute_stdout(
        &transfer_file_cmd(
            "[Resolve]\nDNS=127.0.0.1:5053#cluster.skate\n",
            "/etc/systemd/resolved.conf.d/skate.conf",
        ),
        true,
        true,
    )
    .await?;

    // no-resolv here is to ensure it respects our localhost upstream, otherwise it'll see localhost
    // in /etc/resolv.conf and ignore ours
    conn.execute_stdout(
        &transfer_file_cmd(
            "server=/cluster.skate/127.0.0.1#5053\nno-resolv\ncache-size=0\n",
            "/etc/dnsmasq.d/skate.conf",
        ),
        true,
        true,
    )
    .await?;

    for dns_service in ["dnsmasq", "systemd-resolved"] {
        let _ = conn
            .execute_stdout(&format!("sudo systemctl restart {dns_service}"), true, true)
            .await;
    }

    Ok(())
}

async fn install_oci_hooks(conn: &Box<dyn SshClient>) -> Result<(), Box<dyn Error>> {
    conn.execute_stdout("sudo mkdir -p /etc/containers/oci/hooks.d", true, true)
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
    // serialize to /etc/containers/oci/hooks.d/skatelet-poststart.json
    let serialized = serde_json::to_string(&oci_poststart_hook).unwrap();
    let path = "/etc/containers/oci/hooks.d/skatelet-poststart.json";
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
    let path = "/etc/containers/oci/hooks.d/skatelet-poststop.json";
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

async fn sync_peers(
    conn: &Box<dyn SshClient>,
    cluster_conf: &Cluster,
) -> Result<(), Box<dyn Error>> {
    let peers = cluster_conf
        .nodes
        .iter()
        .filter(|n| n.name != conn.node_name())
        .collect::<Vec<_>>();

    let peers_args = peers
        .iter()
        .map(|p| format!("--peer {}:{}:{}", p.name, p.peer_host, p.subnet_cidr))
        .collect::<Vec<_>>()
        .join(" ");

    conn.execute_stdout(&format!("sudo skatelet peers set {peers_args}"), true, true)
        .await?;
    conn.execute_stdout("sudo skatelet routes", true, true)
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

#[cfg(test)]
mod tests {
    use crate::config::{Cluster, Node};
    use crate::create::node::template_coredns_manifest;
    use k8s_openapi::api::apps::v1::DaemonSet;
    use std::default;

    #[test]
    fn test_should_template_coredns_manifest() {
        let cluster = Cluster {
            name: "".to_string(),
            default_user: None,
            default_key: None,
            nodes: vec![
                Node {
                    name: "node-1".to_string(),
                    host: "".to_string(),
                    peer_host: "10.0.0.1".to_string(),
                    ..Default::default()
                },
                Node {
                    name: "node-2".to_string(),
                    host: "".to_string(),
                    peer_host: "10.0.0.2".to_string(),
                    ..Default::default()
                },
            ],
        };

        let manifest = template_coredns_manifest(&cluster);

        println!("{manifest}");

        let daemonset: DaemonSet = serde_yaml::from_str(&manifest).unwrap();

        let envs = daemonset
            .spec
            .unwrap_or_default()
            .template
            .spec
            .unwrap_or_default()
            .containers[0]
            .env
            .clone()
            .unwrap_or_default();
        let core_file_env = envs.into_iter().find(|e| e.name == "CORE_FILE");
        assert!(core_file_env.is_some());
        let core_file = core_file_env.unwrap();

        let expect = r#"
# What's going on here you might ask? This is to provide at least 2 upstreams to the forward plugin in
# order for it to keep doing healthchecks. It doesnt if there's only 1 upstream.
.:6053 {

}
# serve dns for this node
.:5553 {

    # rewrite name suffix .n-node-1.skate. .cluster.skate.
    # rewrite name suffix .n-node-2.skate. .cluster.skate.
    #...
                   rewrite name suffix .n-node-1.skate. .cluster.skate.
   rewrite name suffix .n-node-2.skate. .cluster.skate.

    # public since other nodes need to reach this
    bind lo 0.0.0.0

    hosts /var/lib/skate/dns/addnhosts
}

svc.cluster.skate:5053 {
    
        bind lo
    
        hosts /var/lib/skate/dns/addnhosts
    
}

pod.cluster.skate:5053 {

    bind lo

    gathersrv pod.cluster.skate. {
      # n-node-1.skate. 1-
                     pod.n-node-1.skate. 0
   pod.n-node-2.skate. 1
    }

    #forward pod.n-node-1.skate. 127.0.0.1:5553 127.0.0.1:6053 {
    #  policy sequential
    #  prefer_udp
    #  health_check 0.1s
    #}
                   forward pod.n-node-1.skate. 10.0.0.1:5553 127.0.0.1:6053 {
       policy sequential
       health_check 0.5s
   }
   forward pod.n-node-2.skate. 10.0.0.2:5553 127.0.0.1:6053 {
       policy sequential
       health_check 0.5s
   }

    cache {
        disable success
    }

    loadbalance round_robin

}
.:5053 {
    bind lo 0.0.0.0
    forward . 8.8.8.8
}
"#;
        assert_eq!(core_file.value.clone().unwrap().as_str(), expect);
    }
}
