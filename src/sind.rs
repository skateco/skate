use crate::deps::SkateDeps;
use crate::errors::SkateError;
use crate::sind::create::{CreateArgs, CreateDeps};
use crate::sind::ports::{PortsArgs, PortsDeps};
use crate::sind::remove::{RemoveArgs, RemoveDeps};
use crate::sind::start::{StartArgs, StartDeps};
use crate::sind::stop::{StopArgs, StopDeps};
use crate::util;
use clap::{Args, Parser, Subcommand};

pub mod create;
pub mod ports;
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

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    #[arg(long, short, long_help = "Name of the cluster to use/create.", default_value_t = String::from("sind"))]
    cluster: String,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(long_about = "Create cluster")]
    Create(CreateArgs),
    #[command(long_about = "Remove cluster")]
    Remove(RemoveArgs),
    #[command(long_about = "Stop cluster nodes")]
    Stop(StopArgs),
    #[command(long_about = "Start cluster nodes")]
    Start(StartArgs),
    #[command(long_about = "Print node ips")]
    Ports(PortsArgs),
}
impl CreateDeps for SkateDeps {}
impl PortsDeps for SkateDeps {}
impl RemoveDeps for SkateDeps {}
impl StartDeps for SkateDeps {}
impl StopDeps for SkateDeps {}
pub async fn sind(deps: SkateDeps) -> Result<(), SkateError> {
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create::create(deps, args).await,
        Commands::Ports(args) => ports::ports(deps, args).await,
        Commands::Remove(args) => remove::remove(deps, args).await,
        Commands::Start(args) => start::start(deps, args).await,
        Commands::Stop(args) => stop::stop(deps, args).await,
    }
}
