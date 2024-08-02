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

