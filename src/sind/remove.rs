use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::create::CONTAINER_LABEL;
use crate::sind::GlobalArgs;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct RemoveArgs {
    #[command(flatten)]
    global: GlobalArgs,
}

pub trait RemoveDeps: With<dyn ShellExec> {}

pub async fn remove<D: RemoveDeps>(deps: D, args: RemoveArgs) -> Result<(), SkateError> {
    let shell_exec: Box<dyn ShellExec> = deps.get();
    let container_ids = shell_exec.exec(
        "docker",
        &[
            "ps",
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
    println!("Removing {} nodes", container_ids.len());
    shell_exec.exec("docker", &[vec!["rm", "-fv"], container_ids].concat())?;

    // remove skate cluster
    let _ = shell_exec.exec(
        "skate",
        &["config", "delete-context", "--yes", &args.global.cluster],
    );
    Ok(())
}
