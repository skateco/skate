use clap::Args;
use std::error::Error;

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(short, long, long_help = "The files that contain the configurations to apply.")]
    filename: Vec<String>,
    #[arg(short, long, long_help("Delete previously applied objects that are not in the set passed to the current invocation."))]
    prune: bool,

}


pub fn apply(_apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    Ok(())
}
