use anyhow::anyhow;
use clap::Args;
use futures::stream::FuturesUnordered;
use crate::config::Config;
use crate::skate::ConfigFileArgs;
use crate::ssh;
use futures::StreamExt;
use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::resource::ResourceType;
use crate::ssh::{SshClients};

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
        let mut cmd: Vec<_> = ["sudo", "podman", "pod", "logs", "--names", "--timestamps"].map(String::from).to_vec();

        if self.follow {
            cmd.push("--follow".to_string());
        }
        if self.tail > 0 {
            let tail = format!("--tail {}", &self.tail);
            cmd.push(tail);
        }
        cmd
    }

    pub fn to_journalctl_args(&self) -> Vec<String> {
        let mut cmd: Vec<_> = ["sudo", "journalctl", "--output", "short-iso-precise", "--quiet"].map(String::from).to_vec();

        if self.follow {
            cmd.push("-f".to_string());
        }
        if self.tail > 0 {
            let tail = format!("-n {}", &self.tail);
            cmd.push(tail);
        }
        cmd.push("-u".to_string());
        cmd

    }
}

pub trait LogsDeps: With<dyn SshManager> {}

pub struct Logs<D: LogsDeps> {
    pub deps: D,
}

impl<D:LogsDeps> Logs<D> {
    pub async fn logs(&self, args: LogArgs) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let mgr = self.deps.get();
        let (conns, errors) = mgr.cluster_connect(config.active_cluster(args.config.context.clone())?).await;


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
                self.log_pod(&conns, name, ns, &args).await
            }
            "deployment" => {
                self.log_child_pods(&conns, ResourceType::Deployment, name, ns, &args).await
            }
            "daemonset" => {
                self.log_child_pods(&conns, ResourceType::DaemonSet, name, ns, &args).await
            }
            "cronjob" => {
                self.log_journalctl(&conns, ResourceType::CronJob, name, ns, &args).await
            }
            _ => {
                Err(anyhow!("Unexpected resource type {}", resource_type).into())
            }
        }
    }

    pub async fn log_pod(&self, conns: &ssh::SshClients, name: &str, _ns: String, args: &LogArgs) -> Result<(), SkateError> {
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

    pub async fn log_child_pods(&self, conns: &SshClients, resource_type: ResourceType, name: &str, ns: String, args: &LogArgs) -> Result<(), SkateError> {
        let log_cmd = args.to_podman_log_args().join(" ");

        let cmd = format!("for id in $(sudo podman pod ls --filter label=skate.io/{}={} --filter label=skate.io/namespace={} -q); do {} $id & done; wait;", resource_type.to_string().to_lowercase(), name, ns, log_cmd);

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);

        let fut: FuturesUnordered<_> = conns.clients.iter().map(|c| c.execute_to_sender(&cmd, tx.clone())).collect();

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                print!("{}", msg);
            }
        });

        let result: Vec<_> = fut.collect().await;

        if result.iter().all(|r| r.is_err()) {
            return Err(format!("{:?}", result.into_iter().map(|r| r.err().unwrap().to_string()).collect::<Vec<String>>()).into());
        }

        for res in result {
            if let Err(e) = res { eprintln!("{}", e) }
        }

        Ok(())
    }
    pub async fn log_journalctl(&self, conns: &SshClients, resource_type: ResourceType, name: &str, ns: String, args: &LogArgs) -> Result<(), SkateError> {
        let mut cmd = args.to_journalctl_args();
        cmd.push(format!("skate-{}-{}.{}.service", resource_type.to_string(), name, ns));

        let cmd = cmd.join(" ");

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);

        let fut: FuturesUnordered<_> = conns.clients.iter().map(|c| c.execute_to_sender(&cmd, tx.clone())).collect();

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                print!("{}", msg);
            }
        });


        let result: Vec<_> = fut.collect().await;

        if result.iter().all(|r| r.is_err()) {
            return Err(format!("{:?}", result.into_iter().map(|r| r.err().unwrap().to_string()).collect::<Vec<String>>()).into());
        }

        for res in result {
            if let Err(e) = res { eprintln!("{}", e) }
        }

        Ok(())
    }
}
