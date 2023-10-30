use std::error::Error;
use clap::{Args, Parser, Subcommand};
use std::process::{Command, ExitStatus};
use semver::{Version, VersionReq};
use thiserror::Error;
use crate::skatelet::UpError::{CommandError, UnsupportedError};
use strum_macros::EnumString;
use crate::skate;
use crate::skate::Os::Linux;

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", version, long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Up(UpArgs),
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
struct UpArgs {}

#[derive(Debug, Args)]
struct ApplyArgs {
    #[arg(short, long, long_help("Pod spec to apply"), required(true))]
    pod_spec: String,
}

pub async fn skatelet() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        Commands::Up(args) => up(args).map_err(|e| e.into()),
        Commands::Apply(args) => apply(args),
        // _ => Ok(())
    }
}

#[derive(Debug, Error, EnumString)]
enum UpError {
    #[error("exit code: {0}")]
    CommandError(ExitStatus, String),
    #[error("{0} is not supported")]
    UnsupportedError(String),
}


fn exec_cmd(command: &str, args: &[&str]) -> Result<String, UpError> {
    let output = Command::new(command)
        .args(args)
        .output()
        .expect("failed to find os");
    if !output.status.success() {
        return Err(CommandError(output.status, String::from_utf8_lossy(&output.stderr).to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}

// up
// - ensures podman is installed and correct version
// later:
// - install wireshare
// - install cron job every minute (skatelet cron)
// - set up systemd scheduler?
// ??
fn up(_up_args: UpArgs) -> Result<(), UpError> {
    let platform = skate::Platform::target();

    let podman_version = exec_cmd("podman", &["--version"]);
    match podman_version {
        Ok(version) => {
            let req = VersionReq::parse(">=1.0.0").unwrap();
            let version = version.split_whitespace().last().unwrap();
            // Check whether it matches 1.3.0 (yes it does)
            let version = Version::parse(version).unwrap();

            if !req.matches(&version) {
                match platform.os {
                    Linux => {
                        // what we gonna do???
                    }
                    _ => {
                        return Err(UnsupportedError("operating system".into()));
                    }
                }
            }
        }
        // instruct on installing newer podman version
        Err(err) => {

// not installed
        }
    }

    println!("{:?}", platform);
    Ok(())
}


fn apply(_apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
