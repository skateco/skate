use std::error::Error;
use clap::{Parser, Subcommand};
use crate::skatelet::apply;
use crate::skatelet::apply::{ApplyArgs};
use crate::skatelet::cni::cni;
use crate::skatelet::delete::{delete, DeleteArgs};
use crate::skatelet::system::{system, SystemArgs};
use crate::skatelet::template::{template, TemplateArgs};

pub const VAR_PATH: &str = "/var/lib/skate";

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
    Delete(DeleteArgs),
    Template(TemplateArgs),
    Cni,
}

pub async fn skatelet() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(args) => apply::apply(args),
        Commands::System(args) => system(args).await,
        Commands::Delete(args) => delete(args),
        Commands::Template(args) => template(args),
        Commands::Cni => {
            cni();
            Ok(())
        },
        // _ => Ok(())
    }
}


