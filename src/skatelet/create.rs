use clap::{Args, Subcommand};

use crate::controllers::cronjob::CronjobController;
use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::Store;

#[derive(Debug, Args, Clone)]
pub struct CreateArgs {
    #[arg(long, short, long_help = "Namespace of the resource.", default_value_t = String::from("default")
    )]
    namespace: String,
    #[command(subcommand)]
    command: CreateCommand,
}

#[derive(Debug, Args, Clone)]
pub struct JobArgs {
    #[arg(
        long,
        long_help("The name of the resource to create a Job from (only cronjob is supported).")
    )]
    pub from: String,
    pub name: String,
    #[arg(short, long, long_help("Wait for the job to complete."))]
    pub wait: bool,
}
#[derive(Debug, Clone, Subcommand)]
pub enum CreateCommand {
    Job(JobArgs),
}

pub trait CreateDeps: With<dyn Store> + With<dyn ShellExec> {}

pub fn create<D: CreateDeps>(deps: D, main_args: CreateArgs) -> Result<(), SkateError> {
    match main_args.command.clone() {
        CreateCommand::Job(args) => create_job(deps, main_args, args),
    }
}

pub fn create_job<D: CreateDeps>(
    deps: D,
    create_args: CreateArgs,
    args: JobArgs,
) -> Result<(), SkateError> {
    let from = args.from.clone();
    let (from_type, from_name) = from.split_once("/").ok_or("invalid --from".to_string())?;
    if from_type == "cronjob" {
        create_job_cronjob(deps, create_args, args, from_name)
    } else {
        Err("only cronjob is supported".to_string().into())
    }
}

pub fn create_job_cronjob<D: CreateDeps>(
    deps: D,
    create_args: CreateArgs,
    args: JobArgs,
    from_name: &str,
) -> Result<(), SkateError> {
    // the pod.yaml is already in the store, so we can just run that

    let execer = With::<dyn ShellExec>::get(&deps);
    let store = With::<dyn Store>::get(&deps);

    let ctrl = CronjobController::new(store, execer);

    ctrl.run(from_name, &create_args.namespace, args.wait)?;
    Ok(())
}
