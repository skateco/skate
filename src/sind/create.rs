use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use clap::Args;
use std::env;

#[derive(Debug, Args, Clone)]
pub struct CreateArgs {}

pub trait CreateDeps: With<dyn ShellExec> {}

const NUM_NODES: usize = 2;

pub async fn create<D: CreateDeps>(deps: D, main_args: CreateArgs) -> Result<(), SkateError> {
    let public_key = ensure_public_key()?;

    let tuples = (1..=NUM_NODES)
        .map(|f| (f, format!("node-{}", f)))
        .collect::<Vec<_>>();

    // remove existing nodes
    let shell_exec: Box<dyn ShellExec> = deps.get();

    println!("Removing existing nodes");
    shell_exec.exec_stdout(
        "docker",
        &[
            vec!["rm", "-fv"],
            tuples
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
        ]
        .concat(),
    )?;

    println!("Creating new nodes");
    for (index, name) in tuples {
        let _ = shell_exec.exec_stdout(
            "docker",
            &[
                "run",
                "-d",
                "--privileged",
                "-p",
                &format!("222{index}:22",),
                "--dns",
                "127.0.0.1",
                "--cgroupns",
                "host",
                "--hostname",
                &name,
                "--tmpfs",
                "/tmp",
                "--tmpfs",
                "/run",
                "--tmpfs",
                "/run/lock",
                "--name",
                &name,
                "ghcr.io/skateco/sind",
            ],
        )?;

        // inject public key in authorized_keys
        let _ = shell_exec.exec_stdout(
            "docker",
            &[
                "exec",
                &name,
                "bash",
                "-c",
                &format!("echo '{}' > /home/skate/.ssh/authorized_keys", public_key),
            ],
        )?;

        println!("Node {} created", name);
    }

    Ok(())
}

pub fn ensure_public_key() -> Result<String, SkateError> {
    Ok("~/.ssh/id_rsa.pub".to_string())
}
