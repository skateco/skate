use clap::{Args, Subcommand};
use std::error::Error;

use std::{io};

use std::io::{Read};


use crate::executor::{DefaultExecutor, Executor};


#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(short, long, long_help("Delete previously applied objects that are not in the set passed to the current invocation."))]
    prune: bool,
    #[command(subcommand)]
    command: StdinCommand,
}


#[derive(Debug, Subcommand)]
pub enum StdinCommand {
    #[command(name = "-", about = "feed manifest yaml via stdin")]
    Stdin {},
}

pub fn apply(apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let manifest = match apply_args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };

    let executor = DefaultExecutor {};
    executor.apply(&manifest)
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    #[arg(short, long, long_help("Number of seconds to wait before hard killing."))]
    termination_grace_period: Option<usize>,
    #[command(subcommand)]
    command: StdinCommand,
}

pub fn remove(args: RemoveArgs) -> Result<(), Box<dyn Error>> {
    let manifest = match args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };

    let executor = DefaultExecutor {};
    executor.remove(&manifest, args.termination_grace_period)
}
