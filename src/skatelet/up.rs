use semver::{Version, VersionReq};
use std::process::ExitStatus;
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
    match podman_version {
        Ok(version) => {
            let req = VersionReq::parse(">=1.0.0").unwrap();
            let version = version.split_whitespace().last().unwrap();
            let version = Version::parse(version).unwrap();

            if !req.matches(&version) {
                match &platform.os {
                    skate::Os::Linux => {
                        // what we gonna do???
                    }
                    _ => {
                        return Err(UnsupportedError("operating system".into()));
                    }
                }
            }
        }
        // instruct on installing newer podman version
        Err(_) => {

// not installed
        }
    }

    println!("{:?}", platform);
    Ok(())
}
