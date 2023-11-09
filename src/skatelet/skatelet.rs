use std::error::Error;
use clap::{Parser, Subcommand};
use crate::skatelet::apply;
use crate::skatelet::apply::{ApplyArgs, remove, RemoveArgs};
use crate::skatelet::system::{system, SystemArgs};
use crate::skatelet::up::{up, UpArgs};

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", version, long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Up(UpArgs),
    Apply(ApplyArgs),
    System(SystemArgs),
    Remove(RemoveArgs)
}

pub async fn skatelet() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Up(args) => up(args).map_err(|e| e.into()),
        Commands::Apply(args) => apply::apply(args),
        Commands::System(args) => system(args).await,
        Commands::Remove(args) => remove(args),
        // _ => Ok(())
    }
}
