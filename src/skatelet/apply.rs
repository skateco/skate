use clap::{Args, Subcommand};
use std::error::Error;
use std::hash::Hash;
use std::{io, process};
use std::fs::File;
use std::io::{Read, Write};
use std::process::Stdio;
use anyhow::anyhow;
use crate::executor::{DefaultExecutor, Executor};
use crate::util::hash_string;

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
