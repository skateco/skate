use clap::{Args, Subcommand};

use std::{io};


use std::io::{Read};
use crate::errors::SkateError;
use crate::executor::{DefaultExecutor};
use crate::resource::SupportedResources;

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(
        short,
        long,
        long_help("Delete previously applied objects that are not in the set passed to the current invocation."
        )
    )]
    prune: bool,
    #[command(subcommand)]
    command: StdinCommand,
}


#[derive(Debug, Subcommand, Clone)]
pub enum StdinCommand {
    #[command(name = "-", about = "feed manifest yaml via stdin")]
    Stdin {},
}

pub fn apply(apply_args: ApplyArgs) -> Result<(), SkateError> {
    let manifest = match apply_args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };

    let executor = DefaultExecutor::new();

    let object: SupportedResources = serde_yaml::from_str(&manifest).expect("failed to deserialize manifest");
    executor.apply(&object)
}

