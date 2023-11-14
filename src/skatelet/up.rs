use semver::{Version, VersionReq};
use std::process::ExitStatus;
use anyhow::anyhow;
use clap::Args;
use strum_macros::EnumString;
use thiserror::Error;
use crate::{skate};
use crate::skate::exec_cmd;
use crate::skatelet::up::UpError::UnsupportedError;

#[derive(Debug, Args)]
pub struct UpArgs {}

#[derive(Debug, Error, EnumString)]
pub enum UpError {
    #[error("exit code: {0}")]
    CommandError(ExitStatus, String),
    #[error("{0} is not supported")]
    UnsupportedError(String),
}

// up
// - ensures podman is installed and correct version
// later:
// - install wireshare
// - install cron job every minute (skatelet cron)
// - set up systemd scheduler?
// ??
pub fn up(_up_args: UpArgs) -> Result<(), UpError> {
    let platform = skate::Platform::target();

    let podman_version = exec_cmd("podman", &["--version"]);

    Ok(())
}
