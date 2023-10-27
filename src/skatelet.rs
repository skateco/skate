use std::error::Error;
use clap::{Args, Command, Parser, Subcommand};

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
        Commands::Up(args) => up(args),
        Commands::Apply(args) => apply(args),
        // _ => Ok(())
    }
}


// up
// - provisions ssh access
// - ensures podman is installed and correct version
// later:
// - install wireshare
// - install cron job every minute (skatelet cron)
// - set up systemd scheduler?
// ??
fn up(_up_args: UpArgs) -> Result<(), Box<dyn Error>> {
    // ensure ssh public key provided

    Ok(())
}

fn apply(_apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
