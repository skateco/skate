#![allow(unused)]

use std::error::Error;
use clap::{Args, Command, Parser, Subcommand};
use serde_yaml;
use serde::Deserialize;
use tokio;

#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
struct HostFileArgs {
    #[arg(env = "SKATE_HOSTS_FILE", long, long_help = "The files that contain the list of hosts.", default_value = "~/.hosts.yaml")]
    hosts_file: String,
}

#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    grace_period: i32,
    #[command(flatten)]
    hosts: HostFileArgs,
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Apply(apply_args) => apply(apply_args),
        _ => Ok(())
    }
}

fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let hosts = read_hosts(args.hosts.hosts_file)?;
    // let merged_config = read_config(args.filename)?;
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}

#[derive(Deserialize)]
struct Host {
    host: String,
}

#[derive(Deserialize)]
struct Hosts {
    hosts: Vec<Host>,
}

fn read_hosts(hosts_file: String) -> Result<Hosts, Box<dyn Error>> {
    let f = std::fs::File::open(".hosts.yaml")?;
    let data: Hosts = serde_yaml::from_reader(f)?;
    Ok(data)
}