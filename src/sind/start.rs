use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::create::CONTAINER_LABEL;
use crate::sind::GlobalArgs;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct StartArgs {
    #[command(flatten)]
    global: GlobalArgs,
}

pub trait StartDeps: With<dyn ShellExec> {}

pub async fn start<D: StartDeps>(deps: D, _: StartArgs) -> Result<(), SkateError> {
    let shell_exec: Box<dyn ShellExec> = deps.get();
    let container_ids = shell_exec.exec(
        "docker",
        &[
            "ps",
            "-a",
            "-q",
            "--filter",
            &format!("label={}", CONTAINER_LABEL),
        ],
    )?;
    let container_ids = container_ids
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if container_ids.is_empty() {
        return Ok(());
    }
    println!("Starting {} nodes", container_ids.len());
    shell_exec.exec("docker", &[vec!["start"], container_ids].concat())?;
    Ok(())
}
