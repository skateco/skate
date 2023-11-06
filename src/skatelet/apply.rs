use clap::{Args, Subcommand};
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::{io, process};
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::io::{Read, Write};
use std::process::Stdio;
use anyhow::anyhow;
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
    let file_path = match apply_args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer);

            let file_path = format!("/tmp/skate-{}.yaml", hash_string(&buffer));
            let mut file = File::create(file_path.clone()).expect("failed to open file for manifests");
            file.write_all(buffer.as_ref()).expect("failed to write manifest to file");
            file_path
        }
    };


    let output = process::Command::new("podman")
        .args(["play", "kube", "--replace", &file_path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .output()

        .expect("failed to run command");
    if !output.status.success() {
        return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
    }

    Ok(())
}
