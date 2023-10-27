use std::error::Error;
use clap::Args;
use crate::skate::HostFileArgs;


#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    grace_period: i32,
    #[command(flatten)]
    hosts: HostFileArgs,
}

pub fn apply(args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let hosts = crate::skate::read_hosts(args.hosts.hosts_file)?;
    let merged_config = crate::skate::read_config(args.filename)?; // huge
    // let game_plan = schedule(merged_config, hosts)?;
    // game_plan.play()
    Ok(())
}
