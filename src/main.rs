#![allow(unused)]

use std::error::Error;
use clap::{Args, Parser, Subcommand};
use tokio;

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
struct ApplyArgs {
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(apply_args) => {


        }
    }
    Ok(())
}
