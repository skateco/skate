use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::skatelet::services::dns::DnsService;
use crate::skatelet::skatelet::log_panic;
use crate::skatelet::VAR_PATH;
use clap::{Args, Subcommand};
use std::panic;

#[derive(Debug, Subcommand)]
pub enum Command {
    Add(AddArgs),
    Remove(RemoveArgs),
    Enable(EnableArgs),
    Reload,
}

#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    command: Command,
}

pub trait DnsDeps: With<dyn ShellExec> {}

pub struct Dns<D: DnsDeps> {
    pub deps: D,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    container_id: String,
    ip: Option<String>,
}

#[derive(Debug, Args)]
pub struct EnableArgs {
    container_id: String,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    #[arg(long, long_help = "The container to remove dns entry for")]
    pub container_id: Option<String>,
    #[arg(long, long_help = "The pod to remove dns entry for")]
    pub pod_id: Option<String>,
}

impl<D: DnsDeps> Dns<D> {
    pub fn dns(&self, args: DnsArgs) -> Result<(), SkateError> {
        panic::set_hook(Box::new(log_panic));

        let execer = With::<dyn ShellExec>::get(&self.deps);
        let svc = DnsService::new(&format!("{VAR_PATH}/dns"), &execer);
        match args.command {
            Command::Add(add_args) => svc.add(add_args.container_id, add_args.ip),
            Command::Remove(remove_args) => {
                svc.remove(remove_args.container_id, remove_args.pod_id)
            }
            Command::Enable(enable_args) => svc.wait_and_enable_healthy(enable_args.container_id),
            Command::Reload => svc.reload(),
        }
    }
}
