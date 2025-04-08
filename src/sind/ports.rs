use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::create::CONTAINER_LABEL;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct PortsArgs {}

pub trait PortsDeps: With<dyn ShellExec> {}

pub async fn ports<D: PortsDeps>(deps: D, _: PortsArgs) -> Result<(), SkateError> {
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

    for id in container_ids {
        let ports = shell_exec.exec("docker", &["port", id, "22"])?;

        println!("{}", ports.lines().next().unwrap_or(&"".to_string()));
    }

    Ok(())
}
