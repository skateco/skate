use std::error::Error;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::{Itertools, min};
use semver::{Version, VersionReq};
use crate::config::{Config, Node};
use crate::skate::{ConfigFileArgs, Distribution, Os};
use crate::ssh::node_connection;
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
            };
            config.clusters[cluster_index].nodes.push(node.clone());
            (true, node)
        }
    };

    let conn = node_connection(&config.clusters[cluster_index], &node).await.map_err(|e| -> Box<dyn Error> { anyhow!("{}", e).into() })?;
    let info = conn.get_node_system_info().await?;
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

    state.reconcile_node(&info)?;

    state.persist()?;

    if new {
        config.persist(Some(args.config.skateconfig))?;
    }

    Ok(())
}