use std::error::Error;
use clap::{Args, Command, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Up(UpArgs),
}

#[derive(Debug, Args)]
struct UpArgs {}

pub async fn skatelet() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Up(up_args) => up(up_args),
        // _ => Ok(())
    }
}


fn up(_up_args: UpArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}