use anyhow::anyhow;
use clap::Args;
use futures::stream::FuturesUnordered;
use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh;
use futures::StreamExt;
use crate::errors::SkateError;
use crate::resource::ResourceType;

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
    #[arg(name = "POD | TYPE/NAME")]
    identifier: String
}

impl LogArgs {
    pub fn to_podman_log_args(&self) -> Vec<String> {
        let mut cmd: Vec<_> = ["sudo", "podman", "pod", "logs", "--names"].map(String::from).to_vec();

        if self.follow {
            cmd.push("--follow".to_string());
        }
        if self.tail > 0 {
            let tail = format!("--tail {}", &self.tail);
            cmd.push(tail);
        }
        cmd
    }
}

pub async fn logs(args: LogArgs) -> Result<(), SkateError> {
    let config = Config::load(Some(args.config.skateconfig.clone()))?;
    let (conns, errors) = ssh::cluster_connections(config.active_cluster(args.config.context.clone())?).await;


    if conns.is_none() {
        if errors.is_some() {
            return Err(anyhow!(errors.unwrap().to_string()).into());
        }
        println!("No connections found");
        return Ok(());
    }

    if errors.is_some() {
        eprintln!("{}", errors.as_ref().unwrap())
    }

    let conns = conns.unwrap();

    let name = args.identifier.clone();

    let ns = args.namespace.clone().unwrap_or("default".to_string());

    let (resource_type, name) = name.split_once("/").unwrap_or(("pod", &name));


    match resource_type {
        "pod" => {
            log_pod(&conns, name, ns, &args).await
        }
        "deployment" => {
            log_child_pods(&conns, ResourceType::Deployment, name, ns, &args).await
        }
        "daemonset" => {
            log_child_pods(&conns, ResourceType::DaemonSet, name, ns, &args).await
        }
        "cronjob" => {
            log_child_pods(&conns, ResourceType::CronJob, name, ns, &args).await
        }
        _ => {
            Err(anyhow!("Unexpected resource type {}", resource_type).into())
        }
    }
}

pub async fn log_pod(conns: &ssh::SshClients, name: &str, _ns: String, args: &LogArgs) -> Result<(), SkateError> {
    let mut cmd = args.to_podman_log_args();

    cmd.push(name.to_string());

    let cmd = cmd.join(" ");

    let fut: FuturesUnordered<_> = conns.clients.iter().map(|c| c.execute_stdout(&cmd, false, false)).collect();

    let result: Vec<_> = fut.collect().await;

    if result.iter().all(|r| r.is_err()) {
        return Err(format!("{:?}", result.into_iter().map(|r| r.err().unwrap().to_string()).collect::<Vec<String>>()).into());
    }

    for res in result {
        if let Err(e) = res { eprintln!("{}", e) }
    }

    Ok(())
}

pub async fn log_child_pods(conns: &ssh::SshClients, resource_type: ResourceType, name: &str, ns: String, args: &LogArgs) -> Result<(), SkateError> {
    let mut cmd = args.to_podman_log_args();

    cmd.push(format!("$(sudo podman pod ls --filter label=skate.io/{}={} --filter label=skate.io/namespace={} -q)", resource_type.to_string().to_lowercase(), name, ns));


    let cmd = cmd.join(" ");

    let fut: FuturesUnordered<_> = conns.clients.iter().map(|c| c.execute_stdout(&cmd, false, false)).collect();

    let result: Vec<_> = fut.collect().await;

    if result.iter().all(|r| r.is_err()) {
        return Err(format!("{:?}", result.into_iter().map(|r| r.err().unwrap().to_string()).collect::<Vec<String>>()).into());
    }

    for res in result {
        if let Err(e) = res { eprintln!("{}", e) }
    }

    Ok(())
}
