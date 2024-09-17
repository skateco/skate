use clap::{Args, Subcommand};
use std::error::Error;

use std::{io};


use std::io::{Read};
use crate::controllers::cronjob::CronjobController;
use crate::executor::{DefaultExecutor, Executor};
use crate::filestore::FileStore;
use crate::skatelet::apply::StdinCommand;

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
        long_help("The name of the resource to create a Job from (only cronjob is supported)."
        )
    )]
    pub from: String,
    pub name: String,
    #[arg(short, long, long_help("Wait for the job to complete."))]
    pub wait: bool
}
#[derive(Debug, Clone, Subcommand)]
pub enum CreateCommand {
    Job(JobArgs)
}


pub fn create(main_args: CreateArgs) -> Result<(), Box<dyn Error>> {
    match main_args.command.clone() {
        CreateCommand::Job(args) => create_job(main_args, args)
    }
}

pub fn create_job(create_args: CreateArgs, args: JobArgs) -> Result<(), Box<dyn Error>> {
    let from = args.from.clone();
    let (from_type, from_name) = from.split_once("/").ok_or("invalid --from")?;
    if from_type == "cronjob" {
        create_job_cronjob(create_args, args, from_name)
    } else {
        Err("only cronjob is supported".into())
    }
}

pub fn create_job_cronjob(create_args: CreateArgs, args: JobArgs, from_name:&str) -> Result<(), Box<dyn Error>> {
    // the pod.yaml is already in the store, so we can just run that

    let ctrl = CronjobController::new(FileStore::new());

    ctrl.run(from_name, &create_args.namespace, args.wait)
}

