use std::fs::OpenOptions;
use std::path::PathBuf;
use anyhow::anyhow;
use clap::{Args};
use crate::errors::SkateError;
use crate::skatelet::skatelet::VAR_PATH;

#[derive(Clone, Debug, Args)]
pub struct CordonArgs {}


pub fn cordon(args: CordonArgs) -> Result<(), SkateError> {
    let path = PathBuf::from(VAR_PATH).join("CORDON");
    let _ = OpenOptions::new().create(true).truncate(true).write(true).open(path).map_err(|e| anyhow!(e).context("failed to create cordon file"))?;
    Ok(())
}

#[derive(Clone, Debug, Args)]
pub struct UncordonArgs{}

pub fn uncordon(args: UncordonArgs) -> Result<(), SkateError> {
    let path = PathBuf::from(VAR_PATH).join("CORDON");
    std::fs::remove_file(path)?;
    Ok(())
}

pub fn is_cordoned() -> bool {
    let path = PathBuf::from(VAR_PATH).join("CORDON");
    path.exists()
}

