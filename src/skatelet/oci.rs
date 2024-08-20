use std::error::Error;
use std::process::{exit, Command, Stdio};
use clap::{Args, Subcommand};
use log::{error, info};
use strum_macros::EnumString;
use crate::skatelet::dns;
use crate::util::spawn_orphan_process;

#[derive(EnumString, Debug, Subcommand)]
pub enum Commands {
    Poststart,
    Poststop,
}
#[derive(Debug, Args)]
pub struct OciArgs {
    #[command(subcommand)]
    pub commands: Commands,
}

pub(crate) fn oci(args: OciArgs) -> Result<(), Box<dyn Error>> {
    let result = match args.commands {
        Commands::Poststart => post_start(),
        Commands::Poststop => post_stop(),
    };

    match result {
        Ok(_) => {
            info!("success");
            Ok(())
        }
        Err(e) => {
            error!("{}", e);
            Err(e)
        }
    }
}

fn post_start() -> Result<(), Box<dyn Error>> {
    info!("poststart");
    let id = container_id()?;
    spawn_orphan_process("skatelet", &["dns", "add", &id]);
    Ok(())
}

fn post_stop() -> Result<(), Box<dyn Error>> {
    info!("poststop");
    let id = container_id()?;
    dns::remove(id)
}

fn container_id() -> Result<String, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    let container_dir = cwd.parent().ok_or_else(|| "no parent dir")?;
    let container_id = container_dir.file_name().ok_or_else(|| "no dir name")?;
    Ok(container_id.to_string_lossy().to_string())
}