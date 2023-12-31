use std::error::Error;
use clap::{Parser, Subcommand};
use crate::skatelet::apply;
use crate::skatelet::apply::{ApplyArgs, remove, RemoveArgs};
use crate::skatelet::cni::cni;
use crate::skatelet::ocihooks::{HookArgs, oci};
use crate::skatelet::system::{system, SystemArgs};

pub const VAR_PATH: &str = "/var/lib/skatelet";

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", version, long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Apply(ApplyArgs),
    System(SystemArgs),
    Remove(RemoveArgs),
    Cni,
    Oci(HookArgs)
}

pub async fn skatelet() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(args) => apply::apply(args),
        Commands::System(args) => system(args).await,
        Commands::Remove(args) => remove(args),
        Commands::Cni => {
            cni();
            Ok(())
        },
        Commands::Oci(args) => oci(args)
        // _ => Ok(())
    }
}


