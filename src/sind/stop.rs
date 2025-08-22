use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::GlobalArgs;
use crate::sind::create::CONTAINER_LABEL;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct StopArgs {
    #[command(flatten)]
    global: GlobalArgs,
}

pub trait StopDeps: With<dyn ShellExec> {}

pub async fn stop<D: StopDeps>(deps: D, _: StopArgs) -> Result<(), SkateError> {
    let shell_exec: Box<dyn ShellExec> = deps.get();
    let container_ids = shell_exec.exec(
        "docker",
        &[
            "ps",
            "-q",
            "--filter",
            &format!("label={}", CONTAINER_LABEL),
        ],
        None,
    )?;
    let container_ids = container_ids
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if container_ids.is_empty() {
        return Ok(());
    }
    println!("Stopping {} nodes", container_ids.len());
    shell_exec.exec("docker", &[vec!["stop"], container_ids].concat(), None)?;
    Ok(())
}
