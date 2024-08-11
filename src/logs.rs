use std::error::Error;
use std::process;
use std::process::Stdio;
use anyhow::anyhow;
use clap::Args;
use futures::stream::FuturesUnordered;
use crate::config::Config;
use crate::create::CreateCommands;
use crate::skate::ConfigFileArgs;
use crate::ssh;
use futures::StreamExt;

#[derive(Debug, Args)]
#[command(arg_required_else_help(true))]
pub struct LogArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(short, long, long_help = "Specify if the logs should be streamed.")]
    pub follow: bool,
    #[arg(
        short, default_value_t = - 1, long, long_help = "Lines of recent log file to display. Defaults to -1."
    )]
    pub tail: i32,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[arg(trailing_var_arg = true, name = "POD | TYPE/NAME")]
    var_args: Vec<String>,
}

pub async fn logs(args: LogArgs) -> Result<(), Box<dyn Error>> {

    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.current_cluster()?).await;

    if errors.is_some() {
        eprintln!("{}", errors.as_ref().unwrap())
    }

    if conns.is_none() {
        if errors.is_some() {
            return Err(anyhow!(errors.unwrap().to_string()).into());
        }
        println!("No connections found");
        return Ok(());
    }

    let conns = conns.unwrap();

    let name = args.var_args.first();
    if name.is_none() {
        return Err("No resource name provided".into());
    }

    let name = name.unwrap();
    let ns = args.namespace.unwrap_or("default".to_string());

    let cmd = format!("sudo podman logs {}", name);
    let fut: FuturesUnordered<_> = conns.clients.iter().map(|c| c.execute_stdout(&cmd)).collect();

    let result: Vec<_> = fut.collect().await;

    for res in result {
        match res {
            Err(e) => eprintln!("{}", e),
            _ => {}
        }
    }
    Ok(())
}