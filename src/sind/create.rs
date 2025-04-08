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
}

pub trait CreateDeps: With<dyn ShellExec> {}

pub const NUM_NODES: usize = 2;
pub const CONTAINER_LABEL: &str = "io.github.skateco.sind=true";

pub async fn create<D: CreateDeps>(deps: D, main_args: CreateArgs) -> Result<(), SkateError> {
    let public_key = ensure_file(&main_args.ssh_public_key)?;
    let public_key_contents = std::fs::read_to_string(&public_key)
        .map_err(|_| format!("Failed to read public key from {}", public_key))?;
    let private_key = ensure_file(&main_args.ssh_private_key)?;

    let tuples = (1..=NUM_NODES)
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
        )?;
    }

    println!("Creating new nodes");
    for (index, name) in &tuples {
        let _ = shell_exec.exec_stdout(
            "docker",
            &[
                "run",
                "-d",
                "--privileged",
                "-p",
                &format!("127.0.0.1:222{index}:22",),
                "--dns",
                "127.0.0.1",
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
                "ghcr.io/skateco/sind",
            ],
        )?;

        // inject public key in authorized_keys
        let _ = shell_exec.exec_stdout(
            "docker",
            &[
                "exec",
                &name,
                "bash",
                "-c",
                &format!(
                    "echo '{}' > /home/skate/.ssh/authorized_keys",
                    public_key_contents
                ),
            ],
        )?;

        println!("Node {} created", name);
    }

    // create skate cluster if not exists
    let clusters = shell_exec.exec("skate", &["config", "get-clusters"])?;
    let cluster_exists = clusters
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .contains(main_args.global.cluster.as_str());

    if !cluster_exists {
        // create cluster
        println!("creating cluster {}", main_args.global.cluster);
        let _ =
            shell_exec.exec_stdout("skate", &["create", "cluster", &main_args.global.cluster])?;
    }

    // use cluster as context
    let _ = shell_exec.exec_stdout(
        "skate",
        &["config", "use-context", &main_args.global.cluster],
    )?;

    let nodes = shell_exec.exec("skate", &["config", "get-nodes"])?;
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
        let peer_host = shell_exec.exec(
            "docker",
            &[
                "inspect",
                "-f",
                "{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                &name,
            ],
        )?;

        // wait for port to open
        let ssh_port = &format!("222{index}");

        let ssh_port_u16 = ssh_port
            .parse::<u16>()
            .map_err(|_| format!("Failed to parse port {}", ssh_port))?;

        let mut attempt = 0;

        // cargo run --bin skate create node --name node-$f --host 127.0.0.1 --peer-host $peer_host --port 222$f --subnet-cidr "20.${f}.0.0/16" --key $SSH_PRIVATE_KEY --user skate
        let _ = shell_exec.exec_stdout(
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
                &ssh_port,
                "--subnet-cidr",
                &format!("20.{index}.0.0/16"),
                "--key",
                &private_key,
                "--user",
                "skate",
            ],
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
