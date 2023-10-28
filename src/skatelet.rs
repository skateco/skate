use std::error::Error;
use clap::{Args, Parser, Subcommand};
use std::process::{Command, ExitStatus, Output};
use thiserror::Error;
use crate::skatelet::Os::{Darwin, Linux, Unknown};
use crate::skatelet::UpError::CommandError;
use strum_macros::EnumString;


#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
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
}

#[derive(Debug)]
enum Os {
    Unknown,
    Linux,
    Darwin,
}


fn exec(command: &str, args: Option<Vec<&str>>) -> Result<String, UpError> {
    let output = Command::new(command)
        .args(args.unwrap_or(vec![]))
        .output()
        .expect("failed to find os");
    if !output.status.success() {
        return Err(CommandError(output.status, String::from_utf8_lossy(&output.stderr).to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into())
}

// up
// - ensures podman is installed and correct version
// later:
// - install wireshare
// - install cron job every minute (skatelet cron)
// - set up systemd scheduler?
// ??
fn up(_up_args: UpArgs) -> Result<(), UpError> {
    let os = exec("uname", Some(vec!["-s"]))?;

    let os = match os.to_lowercase() {
        s if s.starts_with("linux") => Linux,
        s if s.starts_with("darwin") => Darwin,
        _ => Unknown
    };
    println!("{:?}", os);

    Ok(())
}


fn apply(_apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
