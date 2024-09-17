use std::error::Error;
use std::{process, thread};
use std::panic::PanicInfo;
use clap::{Parser, Subcommand};
use log::{error, LevelFilter};
use strum::AsStaticRef;
use strum_macros::{AsStaticStr, IntoStaticStr};
use syslog::{BasicLogger, Facility, Formatter3164};
use crate::skatelet::apply;
use crate::skatelet::apply::{ApplyArgs};
use crate::skatelet::cni::cni;
use crate::skatelet::create::{create, CreateArgs};
use crate::skatelet::delete::{delete, DeleteArgs};
use crate::skatelet::dns::{dns, DnsArgs};
use crate::skatelet::ipvs::{ipvs, IpvsArgs};
use crate::skatelet::oci::{oci, OciArgs};
use crate::skatelet::system::{system, SystemArgs};
use crate::skatelet::template::{template, TemplateArgs};

pub const VAR_PATH: &str = "/var/lib/skate";

#[derive(Debug, Parser)]
#[command(name = "skatelet")]
#[command(about = "Skatelet", version, long_about = "Skate agent to be run on nodes", arg_required_else_help = true)]
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
    Cni,
    Oci(OciArgs),
    Ipvs(IpvsArgs),
    Create(CreateArgs),
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

pub async fn skatelet() -> Result<(), Box<dyn Error>> {

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
        .map(|()| log::set_max_level(LevelFilter::Debug))?;


    let result = match args.command {
        Commands::Apply(args) => apply::apply(args),
        Commands::System(args) => system(args).await,
        Commands::Delete(args) => delete(args),
        Commands::Template(args) => template(args),
        Commands::Cni => {
            cni();
            Ok(())
        },
        Commands::Dns(args) => dns(args),
        Commands::Oci(args) => oci(args),
        Commands::Ipvs(args) => ipvs(args),
        Commands::Create(args) => create(args)
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


