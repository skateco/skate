use clap::{Args, Subcommand};
use std::error::Error;

use std::{io};

use std::io::{Read};


use crate::executor::{DefaultExecutor, Executor};
use crate::skate::SupportedResources;
use crate::skate::SupportedResources::Ingress;

use k8s_openapi::api::networking::v1::Ingress as K8sIngress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use log::Metadata;


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
    }
}


pub fn delete_ingress(delete_args: DeleteArgs, resource_args: DeleteResourceArgs) -> Result<(), Box<dyn Error>> {
    let executor = DefaultExecutor::new();

    executor.manifest_delete(Ingress(K8sIngress {
        metadata: ObjectMeta {
            annotations: None,
            creation_timestamp: None,
            deletion_grace_period_seconds: None,
            deletion_timestamp: None,
            finalizers: None,
            generate_name: None,
            generation: None,
            labels: None,
            managed_fields: None,
            name: Some(resource_args.name),
            namespace: Some(resource_args.namespace),
            owner_references: None,
            resource_version: None,
            self_link: None,
            uid: None,
        },
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
