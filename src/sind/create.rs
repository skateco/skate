use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct CreateArgs {}

pub trait CreateDeps: With<dyn ShellExec> {}

pub async fn create<D: CreateDeps>(deps: D, main_args: CreateArgs) -> Result<(), SkateError> {
    Ok(())
}
