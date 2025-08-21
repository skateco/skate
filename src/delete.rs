use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::util::CHECKBOX_EMOJI;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use dialoguer::Confirm;
use itertools::Itertools;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: DeleteCommands,
}

#[derive(Debug, Subcommand)]
pub enum DeleteCommands {
    Node(DeleteResourceArgs),
    Ingress(DeleteResourceArgs),
    Cronjob(DeleteResourceArgs),
    Secret(DeleteResourceArgs),
    Deployment(DeleteResourceArgs),
    Daemonset(DeleteResourceArgs),
    Service(DeleteResourceArgs),
    ClusterIssuer(DeleteResourceArgs),
    Cluster(DeleteClusterArgs),
    Namespace(DeleteNamespaceArgs),
}

#[derive(Debug, Args)]
pub struct DeleteNamespaceArgs {
    name: String,
    #[command(flatten)]
    config: ConfigFileArgs,
}

#[derive(Debug, Args)]
pub struct DeleteResourceArgs {
    name: String,
    #[arg(long, short, global = true, long_help = "Namespace of the resource.", default_value_t = String::from("default"))]
    namespace: String,
    #[command(flatten)]
    config: ConfigFileArgs,
}

#[derive(Debug, Args)]
pub struct DeleteClusterArgs {
    name: String,
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Answer yes to confirmation")]
    pub yes: bool,
}

pub trait DeleteDeps: With<dyn SshManager> {}

pub struct Delete<D: DeleteDeps> {
    pub deps: D,
}

impl<D: DeleteDeps> Delete<D> {
    pub async fn delete(&self, args: DeleteArgs) -> Result<(), SkateError> {
        match args.command {
            DeleteCommands::Node(args) => self.delete_node(args).await?,
            DeleteCommands::Daemonset(args) => {
                self.delete_resource(ResourceType::DaemonSet, args).await?
            }
            DeleteCommands::Deployment(args) => {
                self.delete_resource(ResourceType::Deployment, args).await?
            }
            DeleteCommands::Ingress(args) => {
                self.delete_resource(ResourceType::Ingress, args).await?
            }
            DeleteCommands::Cronjob(args) => {
                self.delete_resource(ResourceType::CronJob, args).await?
            }
            DeleteCommands::Secret(args) => {
                self.delete_resource(ResourceType::Secret, args).await?
            }
            DeleteCommands::Service(args) => {
                self.delete_resource(ResourceType::Service, args).await?
            }
            DeleteCommands::ClusterIssuer(args) => {
                self.delete_resource(ResourceType::ClusterIssuer, args)
                    .await?
            }
            DeleteCommands::Cluster(args) => self.delete_cluster(args).await?,
            DeleteCommands::Namespace(args) => {
                self.delete_resource(
                    ResourceType::Namespace,
                    DeleteResourceArgs {
                        name: args.name.clone(),
                        namespace: args.name,
                        config: args.config,
                    },
                )
                .await?
            }
        }
        Ok(())
    }

    async fn delete_resource(
        &self,
        r_type: ResourceType,
        args: DeleteResourceArgs,
    ) -> Result<(), SkateError> {
        // fetch state for resource type from nodes

        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let ssh_mgr = self.deps.get();
        let (conns, errors) = ssh_mgr
            .cluster_connect(config.active_cluster(args.config.context)?)
            .await;
        if errors.is_some() {
            eprintln!("{}", errors.unwrap())
        }

        if conns.is_none() {
            return Ok(());
        }

        let conns = conns.unwrap();

        let mut results = vec![];
        let mut errors = vec![];

        for conn in conns.clients {
            match conn
                .remove_resource(r_type.clone(), &args.name, &args.namespace)
                .await
            {
                Ok(result) => {
                    if !result.0.is_empty() {
                        result
                            .0
                            .trim()
                            .split("\n")
                            .map(|line| format!("{} - {}", conn.node_name(), line))
                            .for_each(|line| println!("{}", line))
                    }
                    results.push(result)
                }
                Err(e) => errors.push(e.to_string()),
            }
        }

        match errors.is_empty() {
            false => Err(anyhow!("\n{}", errors.join("\n")).into()),
            true => {
                println!(
                    "{} deleted {} {}.{}",
                    CHECKBOX_EMOJI, r_type, args.name, args.namespace
                );
                Ok(())
            }
        }
    }

    async fn delete_node(&self, args: DeleteResourceArgs) -> Result<(), SkateError> {
        let mut config = Config::load(Some(args.config.skateconfig.clone()))?;

        let mut cluster = config.active_cluster(args.config.context.clone())?.clone();

        let find_result = cluster.nodes.iter().find_position(|n| n.name == args.name);

        match find_result {
            Some((p, _)) => {
                cluster.nodes.remove(p);
                config.replace_cluster(&cluster)?;
                config.persist(Some(args.config.skateconfig))
            }
            None => Ok(()),
        }
    }

    async fn delete_cluster(&self, args: DeleteClusterArgs) -> Result<(), SkateError> {
        let mut config = Config::load(Some(args.config.skateconfig.clone()))?;
        let cluster = config
            .clusters
            .iter()
            .find(|c| c.name == args.name)
            .ok_or(anyhow!("cluster not found"))?;

        if !args.yes {
            let confirmation = Confirm::new()
                .with_prompt(format!(
                    "Are you sure you want to delete cluster {}?",
                    args.name
                ))
                .wait_for_newline(true)
                .interact()
                .unwrap();

            if !confirmation {
                return Ok(());
            }
        }
        config.delete_cluster(&cluster.clone())?;
        config.persist(Some(args.config.skateconfig))
    }
}
