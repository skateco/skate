use clap::{Args, Subcommand};
use std::error::Error;

use std::{io};
use std::collections::BTreeMap;

use std::io::{Read};


use crate::executor::{DefaultExecutor, Executor};
use crate::skate::SupportedResources;
use crate::skate::SupportedResources::{CronJob, Ingress};
use k8s_openapi::api::batch::v1::CronJob as K8sCronJob;

use k8s_openapi::api::networking::v1::Ingress as K8sIngress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;


#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(
        short,
        long,
        long_help("Delete previously applied objects that are not in the set passed to the current invocation."
        )
    )]
    prune: bool,
    #[command(subcommand)]
    command: StdinCommand,
}


#[derive(Debug, Subcommand, Clone)]
pub enum StdinCommand {
    #[command(name = "-", about = "feed manifest yaml via stdin")]
    Stdin {},
}

pub fn apply(apply_args: ApplyArgs) -> Result<(), Box<dyn Error>> {
    let manifest = match apply_args.command {
        StdinCommand::Stdin {} => {
            let mut stdin = io::stdin();
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };

    let executor = DefaultExecutor::new();
    executor.apply(&manifest)
}

#[derive(Debug, Args, Clone)]
pub struct DeleteResourceArgs {
    #[arg(long, long_help = "Name of the resource.")]
    pub name: String,
    #[arg(long, long_help = "Name of the resource.")]
    pub namespace: String,
}

#[derive(Debug, Subcommand, Clone)]
pub enum DeleteResourceCommands {
    #[command(flatten)]
    StdinCommand(StdinCommand),
    Ingress(DeleteResourceArgs),
    Cronjob(DeleteResourceArgs)
}

#[derive(Debug, Args, Clone)]
pub struct DeleteArgs {
    #[arg(short, long, long_help("Number of seconds to wait before hard killing."))]
    termination_grace_period: Option<usize>,
    #[command(subcommand)]
    command: DeleteResourceCommands,
}

pub fn delete(args: DeleteArgs) -> Result<(), Box<dyn Error>> {
    match &args.command {
        DeleteResourceCommands::Ingress(resource_args) => delete_ingress(args.clone(), resource_args.clone()),
        DeleteResourceCommands::StdinCommand(_) => delete_stdin(args),
        DeleteResourceCommands::Cronjob(resource_args) => delete_cronjob(args.clone(), resource_args.clone()),
    }
}


pub fn delete_ingress(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    let executor = DefaultExecutor::new();
    let mut meta = ObjectMeta::default();
    meta.name = Some(resource_args.name.clone());
    meta.namespace = Some(resource_args.namespace.clone());
    meta.labels = Some(BTreeMap::from([
        ("skate.io/name".to_string(), resource_args.name),
        ("skate.io/namespace".to_string(), resource_args.namespace),
    ]));

    executor.manifest_delete(Ingress(K8sIngress {
        metadata: meta,
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_cronjob(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    let executor = DefaultExecutor::new();
    let mut meta = ObjectMeta::default();
    meta.name = Some(resource_args.name.clone());
    meta.namespace = Some(resource_args.namespace.clone());
    meta.labels = Some(BTreeMap::from([
        ("skate.io/name".to_string(), resource_args.name),
        ("skate.io/namespace".to_string(), resource_args.namespace),
    ]));

    executor.manifest_delete(CronJob(K8sCronJob {
        metadata: meta,
        spec: None,
        status: None,
    }), delete_args.termination_grace_period)
}

pub fn delete_stdin(args: DeleteArgs) -> Result<(), Box<dyn Error>> {
    let manifest = {
        let mut stdin = io::stdin();
        let mut buffer = String::new();
        stdin.read_to_string(&mut buffer)?;
        buffer
    };

    let executor = DefaultExecutor::new();
    let object: SupportedResources = serde_yaml::from_str(&manifest).expect("failed to deserialize manifest");
    executor.manifest_delete(object, args.termination_grace_period)
}
