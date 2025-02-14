use crate::deps::Deps;
use crate::errors::SkateError;
use crate::skatelet::apply;
use crate::skatelet::apply::{ApplyArgs, ApplyDeps};
use crate::skatelet::cordon::{cordon, uncordon, CordonArgs, UncordonArgs};
use crate::skatelet::create::{create, CreateArgs, CreateDeps};
use crate::skatelet::delete::{DeleteArgs, DeleteDeps, Deleter};
use crate::skatelet::dns::{Dns, DnsArgs, DnsDeps};
use crate::skatelet::ipvs::{IPVSDeps, IpvsArgs, IPVS};
use crate::skatelet::oci::{oci, OciArgs};
use crate::skatelet::system::{system, SystemArgs, SystemDeps};
use crate::skatelet::template::{template, TemplateArgs};
use crate::util;
use anyhow::anyhow;
use clap::{Parser, Subcommand};
use log::{error, LevelFilter};
use std::panic::PanicInfo;
use std::{process, thread};
use strum_macros::IntoStaticStr;
use syslog::{BasicLogger, Facility, Formatter3164};

pub const VAR_PATH: &str = "/var/lib/skate";

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(
    about = "Skatelet",
    version,
    long_about = "Skate agent to be run on nodes",
    arg_required_else_help = true
)]
#[clap(version = util::version(false), long_version = util::version(true))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand, IntoStaticStr)]
enum Commands {
    Apply(ApplyArgs),
    System(SystemArgs),
    Delete(DeleteArgs),
    Template(TemplateArgs),
    Dns(DnsArgs),
    Oci(OciArgs),
    Ipvs(IpvsArgs),
    Create(CreateArgs),
    Cordon(CordonArgs),
    Uncordon(UncordonArgs),
}

pub fn log_panic(info: &PanicInfo) {
    let thread = thread::current();
    let thread = thread.name().unwrap_or("<unnamed>");

    let msg = match info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match info.payload().downcast_ref::<String>() {
            Some(s) => &**s,
            None => "Box<Any>",
        },
    };

    match info.location() {
        Some(location) => {
            error!(
                target: "panic", "thread '{}' panicked at '{}': {}:{}",
                thread,
                msg,
                location.file(),
                location.line(),
            );
        }
        None => error!(
            target: "panic",
            "thread '{}' panicked at '{}'",
            thread,
            msg,
        ),
    }
}

impl ApplyDeps for Deps {}
impl SystemDeps for Deps {}
impl CreateDeps for Deps {}
impl DeleteDeps for Deps {}
impl DnsDeps for Deps {}
impl IPVSDeps for Deps {}

pub async fn skatelet() -> Result<(), SkateError> {
    let args = Cli::parse();

    let cmd_name: &'static str = (&args.command).into();
    let formatter = Formatter3164 {
        facility: Facility::LOG_USER,
        hostname: None,
        process: format!("skatelet-{}", cmd_name.to_lowercase()),
        pid: process::id(),
    };
    let logger = match syslog::unix(formatter) {
        Err(e) => return Err(e.into()),
        Ok(logger) => logger,
    };

    log::set_boxed_logger(Box::new(BasicLogger::new(logger)))
        .map(|()| log::set_max_level(LevelFilter::Debug))
        .map_err(|e| anyhow!(e))?;

    let deps = Deps {};

    let result = match args.command {
        Commands::Apply(args) => apply::apply(deps, args),
        Commands::System(args) => system(deps, args).await,
        Commands::Delete(args) => {
            let deleter = Deleter { deps };
            deleter.delete(args)
        }
        // TODO - deps
        Commands::Template(args) => template(args),
        Commands::Dns(args) => {
            let dns = Dns { deps };
            dns.dns(args)
        }
        // TODO - deps
        Commands::Oci(args) => oci(args),
        Commands::Ipvs(args) => {
            let ipvs = IPVS { deps };
            ipvs.ipvs(args)
        }
        Commands::Create(args) => create(deps, args),
        Commands::Cordon(args) => cordon(args),
        Commands::Uncordon(args) => uncordon(args),
        // _ => Ok(())
    };
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("{}", e);
            Err(e)
        }
    }
}
