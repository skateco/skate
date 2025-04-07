use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::sind::create::CONTAINER_LABEL;
use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct IpsArgs {}

pub trait IpsDeps: With<dyn ShellExec> {}

pub async fn ips<D: IpsDeps>(deps: D, main_args: IpsArgs) -> Result<(), SkateError> {
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
    let ips = shell_exec.exec(
        "docker",
        &[
            vec![
                "inspect",
                "-f",
                "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
            ],
            container_ids,
        ]
        .concat(),
    )?;
    println!("{ips}");
    Ok(())
}
