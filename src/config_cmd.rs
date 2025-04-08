use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use dialoguer::Confirm;

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[command(subcommand)]
    command: ConfigCommands,
}

#[derive(Debug, Args)]
pub struct UseContextArgs {
    pub context: String,
}

#[derive(Debug, Args)]
pub struct DeleteContextArgs {
    pub context: String,
    #[arg(long, short, long_help = "Answer yes to confirmation")]
    pub yes: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    GetClusters,
    GetContexts,
    GetNodes,
    CurrentContext,
    UseContext(UseContextArgs),
    DeleteContext(DeleteContextArgs),
}

pub fn config(args: ConfigArgs) -> Result<(), SkateError> {
    match args.command {
        ConfigCommands::GetContexts | ConfigCommands::GetClusters => {
            let config = crate::config::Config::load(Some(args.config.skateconfig.clone()))?;
            println!("NAME");
            for ctx in config.clusters {
                println!("{}", ctx.name)
            }
        }
        ConfigCommands::CurrentContext => {
            let config = crate::config::Config::load(Some(args.config.skateconfig.clone()))?;
            println!("{}", config.current_context.unwrap_or_default())
        }
        ConfigCommands::GetNodes => {
            let config = crate::config::Config::load(Some(args.config.skateconfig.clone()))?;
            let cluster = config.active_cluster(None)?;

            for node in &cluster.nodes {
                println!("{}", node.name);
            }
        }
        ConfigCommands::UseContext(use_context_args) => {
            let mut config = crate::config::Config::load(Some(args.config.skateconfig.clone()))?;
            config
                .clusters
                .iter()
                .any(|c| c.name == use_context_args.context)
                .then_some(())
                .ok_or(anyhow!(
                    "no context exists with the name {}",
                    use_context_args.context
                ))?;
            config.current_context = Some(use_context_args.context.clone());
            config.persist(Some(args.config.skateconfig))?;
            println!(
                "Switched to context \"{}\"",
                use_context_args.context.replace("\"", "")
            );
        }
        ConfigCommands::DeleteContext(delete_context_args) => {
            let mut config = crate::config::Config::load(Some(args.config.skateconfig.clone()))?;
            let cluster = config
                .clusters
                .iter()
                .position(|c| c.name == delete_context_args.context)
                .ok_or(anyhow!(
                    "no context exists with the name {}",
                    delete_context_args.context
                ))?;

            if !delete_context_args.yes {
                let confirmation = Confirm::new()
                    .with_prompt(format!(
                        "Are you sure you want to delete context {}?",
                        delete_context_args.context,
                    ))
                    .wait_for_newline(true)
                    .interact()
                    .unwrap();

                if !confirmation {
                    return Ok(());
                }
            }

            config.clusters.remove(cluster);
            config.persist(Some(args.config.skateconfig))?;
            println!(
                "Deleted context \"{}\"",
                delete_context_args.context.replace("\"", "")
            );
        }
    }
    Ok(())
}
