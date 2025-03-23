use crate::deps::Deps;
use crate::errors::SkateError;
use crate::sind::create::{CreateArgs, CreateDeps};
use crate::util;
use clap::{Parser, Subcommand};

pub mod create;

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "SIND CLI", long_about = None, arg_required_else_help = true, version)]
#[clap(version = util::version(false), long_version = util::version(true))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(long_about = "Create resources")]
    Create(CreateArgs),
}
impl CreateDeps for Deps {}
pub async fn sind(deps: Deps) -> Result<(), SkateError> {
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create::create(deps, args).await,
    }
}
