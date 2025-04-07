use crate::deps::Deps;
use crate::errors::SkateError;
use crate::sind::create::{CreateArgs, CreateDeps};
use crate::sind::ips::{IpsArgs, IpsDeps};
use crate::sind::remove::{RemoveArgs, RemoveDeps};
use crate::util;
use clap::{Parser, Subcommand};

pub mod create;
pub mod ips;
pub mod remove;
pub mod start;
pub mod stop;

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
    #[command(long_about = "Create cluster")]
    Create(CreateArgs),
    #[command(long_about = "Remove cluster")]
    Remove(RemoveArgs),
    // #[command(long_about = "Stop cluster nodes")]
    // Stop(StopArgs),
    // #[command(long_about = "Start cluster nodes")]
    // Start(StartArgs),
    #[command(long_about = "Print node ips")]
    Ips(IpsArgs),
}
impl CreateDeps for Deps {}
impl IpsDeps for Deps {}
impl RemoveDeps for Deps {}
pub async fn sind(deps: Deps) -> Result<(), SkateError> {
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create::create(deps, args).await,
        Commands::Ips(args) => ips::ips(deps, args).await,
        Commands::Remove(args) => remove::remove(deps, args).await,
    }
}
