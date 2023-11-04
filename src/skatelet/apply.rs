use clap::{Args, Subcommand};
use std::error::Error;
use std::hash::Hash;
use std::{process};
use std::process::Stdio;
use anyhow::anyhow;

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(short, long, long_help("Delete previously applied objects that are not in the set passed to the current invocation."))]
    prune: bool,
    #[command(subcommand)]
    command: StdinCommand,
}


#[derive(Debug, Subcommand)]
pub enum StdinCommand {
    #[command(name = "-", about="feed manifest yaml via stdin")]
    Stdin{},
}

pub fn apply(apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    match apply_args.command {
        StdinCommand::Stdin {}=> {
            let output = process::Command::new("podman")
                .args(["play", "kube", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .output()

                .expect("failed to run command");
            if !output.status.success() {
                return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
            }

            Ok(())
        }
    }
}
