use crate::config::Config;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::refresh::{Refresh, RefreshDeps};
use crate::resource::SupportedResources;
use crate::scheduler::{DefaultScheduler, Scheduler};
use crate::skate::ConfigFileArgs;
use anyhow::anyhow;
use clap::Args;
use itertools::{Either, Itertools};
use serde::Deserialize;
use serde_yaml::Value;
use std::error::Error;
use std::io::Read;
use std::{fs, io};

#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct ApplyArgs {
    #[arg(
        short,
        long,
        long_help = "The files that contain the configurations to apply."
    )]
    pub filename: Vec<String>,
    #[arg(long, default_value_t = - 1, long_help = "Period of time in seconds given to the resource to terminate gracefully. Ignored if negative. Set to 1 for \
immediate shutdown.")]
    pub grace_period: i32,
    #[command(flatten)]
    pub config: ConfigFileArgs,
    #[arg(long, long_help = "Will not affect the cluster if set to true")]
    pub dry_run: bool,
}

pub trait ApplyDeps: With<dyn SshManager> + RefreshDeps {}

pub struct Apply<D: ApplyDeps> {
    pub deps: D,
}

impl<D: ApplyDeps> Apply<D> {
    pub async fn apply(deps: &D, args: ApplyArgs) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig))?;
        let objects = read_manifests(args.filename)?;
        Self::apply_supported_resources(deps, &config, objects, args.dry_run).await
    }

    pub async fn apply_self(&self, args: ApplyArgs) -> Result<(), SkateError> {
        Self::apply(&self.deps, args).await
    }

    pub(crate) async fn apply_supported_resources(
        deps: &D,
        config: &Config,
        resources: Vec<SupportedResources>,
        dry_run: bool,
    ) -> Result<(), SkateError> {
        let cluster = config.active_cluster(config.current_context.clone())?;
        let ssh_manager = deps.get();
        let (conns, errors) = ssh_manager.cluster_connect(cluster).await;
        if let Some(e) = errors {
            for e in e.errors {
                eprintln!("{} - {}", e.node_name, e.error)
            }
        };

        if conns.is_none() {
            return Err(anyhow!("failed to create cluster connections").into());
        };

        let objects: Vec<Result<_, _>> = resources.into_iter().map(|sr| sr.fixup()).collect();

        // gather errors
        let (objects, errors): (Vec<_>, Vec<_>) = objects.into_iter().partition_map(|r| match r {
            Ok(o) => Either::Left(o),
            Err(e) => Either::Right(e),
        });

        if !errors.is_empty() {
            for e in errors {
                eprintln!("{}", e);
            }

            return Err(anyhow!("some resources were invalid").into());
        }

        let conns = conns.ok_or("no clients".to_string())?;

        let mut state = Refresh::<D>::refreshed_state(&cluster.name, &conns, config)
            .await
            .expect("failed to refresh state");

        let scheduler = DefaultScheduler {};
        match scheduler
            .schedule(&conns, &mut state, objects, dry_run)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}", e);
                return Err(anyhow!("failed to schedule resources").into());
            }
        }

        Ok(())
    }
}

pub fn read_manifests(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let mut result: Vec<SupportedResources> = Vec::new();

    let num_filenames = filenames.len();

    for filename in filenames {
        let str_file = {
            if num_filenames == 1 && filename == "-" {
                let mut stdin = io::stdin();
                let mut buffer = String::new();
                stdin.read_to_string(&mut buffer)?;
                buffer
            } else {
                fs::read_to_string(filename).expect("failed to read file")
            }
        };
        for document in serde_yaml::Deserializer::from_str(&str_file) {
            let value = Value::deserialize(document).expect("failed to read document");
            if let Value::Mapping(_) = &value {
                result.push(SupportedResources::try_from(&value)?)
            }
        }
    }
    Ok(result)
}
