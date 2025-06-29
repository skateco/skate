use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::GlobalArgs;
use clap::Args;
use itertools::Itertools;

#[derive(Debug, Args, Clone)]
pub struct CreateArgs {
    // force
    #[arg(short, long, long_help = "Force removal of existing containers")]
    force: bool,
    #[command(flatten)]
    global: GlobalArgs,
    #[arg(long, long_help = "SSH private key path", default_value_t= String::from("~/.ssh/id_rsa"))]
    ssh_private_key: String,
    #[arg(long, long_help = "SSH public key path", default_value_t= String::from("~/.ssh/id_rsa.pub"))]
    ssh_public_key: String,
    #[arg(
        long,
        long_help = "Path to skatelet binary to use instead of downloading"
    )]
    skatelet_binary_path: Option<String>,
    #[arg(long, long_help = "Container image to use", default_value_t = String::from("ghcr.io/skateco/sind"))]
    image: String,
    #[arg(
        long,
        long_help = "Number of nodes in the cluster",
        default_value_t = 2
    )]
    nodes: usize,
}

pub trait CreateDeps: With<dyn ShellExec> {}

pub const CONTAINER_LABEL: &str = "io.github.skateco.sind=true";

pub async fn create<D: CreateDeps>(deps: D, main_args: CreateArgs) -> Result<(), SkateError> {
    let public_key = ensure_file(&main_args.ssh_public_key)?;
    let public_key_contents = std::fs::read_to_string(&public_key)
        .map_err(|_| format!("Failed to read public key from {}", public_key))?;
    let private_key = ensure_file(&main_args.ssh_private_key)?;

    let tuples = (1..=main_args.nodes)
        .map(|f| (f, format!("sind-node-{}", f)))
        .collect::<Vec<_>>();

    // remove existing nodes
    let shell_exec: Box<dyn ShellExec> = deps.get();

    if main_args.force {
        println!("Removing existing nodes");
        shell_exec.exec_stdout(
            "docker",
            &[
                vec!["rm", "-fv"],
                tuples
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>(),
            ]
            .concat(),
            None,
        )?;
    }

    println!("Creating new nodes");
    for (index, name) in &tuples {
        let http_port = 8880 + index;
        let ssh_port = 2220 + index;
        shell_exec.exec_stdout(
            "docker",
            &[
                "run",
                "-d",
                "--privileged",
                "-p",
                &format!("127.0.0.1:{ssh_port}:22",),
                "-p",
                &format!("127.0.0.1:{http_port}:80",),
                "--dns",
                "127.0.0.99",
                "--cgroupns",
                "host",
                "--hostname",
                name,
                "--tmpfs",
                "/tmp",
                "--tmpfs",
                "/run",
                "--tmpfs",
                "/run/lock",
                "--label",
                CONTAINER_LABEL,
                "--name",
                name,
                &main_args.image,
            ],
            None,
        )?;

        // inject public key in authorized_keys
        shell_exec.exec_stdout(
            "docker",
            &[
                "exec",
                name,
                "bash",
                "-c",
                &format!(
                    "echo '{}' > /home/skate/.ssh/authorized_keys; chown -R skate:skate /home/skate/.ssh &&  chmod 600 /home/skate/.ssh/authorized_keys",
                    public_key_contents
                ),
            ],
            None,
        )?;

        println!("Node {} created", name);
    }

    // create skate cluster if not exists
    let clusters = shell_exec.exec("skate", &["config", "get-clusters"], None)?;
    let cluster_exists = clusters
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .contains(main_args.global.cluster.as_str());

    if !cluster_exists {
        // create cluster
        println!("creating cluster {}", main_args.global.cluster);
        shell_exec.exec_stdout(
            "skate",
            &["create", "cluster", &main_args.global.cluster],
            None,
        )?;
    }

    // use cluster as context
    shell_exec.exec_stdout(
        "skate",
        &["config", "use-context", &main_args.global.cluster],
        None,
    )?;

    let nodes = shell_exec.exec("skate", &["config", "get-nodes"], None)?;
    let has_nodes = nodes.lines().count() > 0;

    if has_nodes {
        // warn that there are nodes and to remove them or the cluster first
        return Err(format!(
            "There are nodes in the cluster named {}. Please remove them or the cluster first.",
            main_args.global.cluster
        )
        .into());
    }

    // peer_host=$(docker inspect -f '{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}' node-$f)

    for (index, name) in tuples {
        let ssh_port = 2220 + index;

        let peer_host = shell_exec.exec(
            "docker",
            &[
                "inspect",
                "-f",
                "{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                &name,
            ],
            None,
        )?;

        if let Some(skatelet_path) = &main_args.skatelet_binary_path {
            shell_exec.exec_stdout(
                "docker",
                &["exec", &name, "mkdir", "-p", "/var/lib/skate"],
                None,
            )?;

            println!("Copying skatelet binary to node {}", name);
            shell_exec.exec_stdout(
                "docker",
                &[
                    "cp",
                    skatelet_path,
                    &format!("{name}:/usr/local/bin/skatelet"),
                ],
                None,
            )?;
        }

        // wait for port to open

        let mut result: Result<_, _> = Err("never ran".into());
        for _ in 0..10 {
            println!("Attempting to connect to node...");
            result = resolvable("127.0.0.1".into(), ssh_port as u32, 5).await;
            if result.is_ok() {
                break;
            }
            // sleep
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        if result.is_err() {
            return Err(format!(
                "Failed to connect to 127.0.0.1:{} => {:?}",
                ssh_port,
                result.err()
            )
            .into());
        }

        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        shell_exec.exec_stdout(
            "skate",
            &[
                "create",
                "node",
                "--name",
                &name,
                "--host",
                "127.0.0.1",
                "--peer-host",
                &peer_host,
                "--port",
                &ssh_port.to_string(),
                "--subnet-cidr",
                &format!("20.{index}.0.0/16"),
                "--key",
                &private_key,
                "--user",
                "skate",
            ],
            None,
        )?;
    }

    Ok(())
}

pub fn ensure_file(path: &str) -> Result<String, SkateError> {
    let path = shellexpand::tilde(path).to_string();
    if !std::path::Path::new(&path).exists() {
        return Err(format!("File {} does not exist", path).into());
    }
    Ok(path)
}

async fn resolvable(
    ip: String,
    port: u32,
    timeout_seconds: u64,
) -> Result<tokio::net::TcpStream, Box<dyn std::error::Error + Send + Sync>> {
    tokio::time::timeout(
        std::time::Duration::from_secs(timeout_seconds),
        tokio::net::TcpStream::connect(format!("{}:{}", ip, port)),
    )
    .await?
    .map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
}
