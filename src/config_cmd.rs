use crate::errors::SkateError;
use crate::skate::ConfigFileArgs;
use anyhow::anyhow;
use clap::{Args, Subcommand};

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

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    GetClusters,
    GetContexts,
    CurrentContext,
    UseContext(UseContextArgs),
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
    }
    Ok(())
}
