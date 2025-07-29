use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::skatelet::database::peer::list_peers;
use clap::Args;
use std::net::ToSocketAddrs;

pub trait RoutesDeps: With<dyn ShellExec> + WithDB {}

pub struct Routes<D: RoutesDeps> {
    pub deps: D,
}

#[derive(Clone, Debug, Args)]
pub struct RoutesArgs {}

impl<D: RoutesDeps> Routes<D> {
    pub async fn routes(&self, _args: RoutesArgs) -> Result<(), SkateError> {
        // list all peers
        let db = self.deps.get_db();
        let peers = list_peers(&db).await?;
        let exec: Box<dyn ShellExec> = self.deps.get();

        for peer in peers {
            let ip = format!("{}:22", peer.host)
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap()
                .ip()
                .to_string();
            Self::exec_or_log_err(
                &exec,
                "ip",
                &["route", "add", &peer.subnet_cidr, "via", &ip],
            );
        }

        Self::exec_or_log_err(&exec, "modprobe", &["--", "ip_vs"]);
        Self::exec_or_log_err(&exec, "modprobe", &["--", "ip_vs_rr"]);
        Self::exec_or_log_err(&exec, "modprobe", &["--", "ip_vs_wrr"]);
        Self::exec_or_log_err(&exec, "modprobe", &["--", "ip_vs_sh"]);
        Self::exec_or_log_err(&exec, "sysctl", &["-w", "net.ipv4.ip_forward=1"]);
        Self::exec_or_log_err(&exec, "sysctl", &["fs.inotify.max_user_instances=1280"]);
        Self::exec_or_log_err(&exec, "sysctl", &["fs.inotify.max_user_watches=655360"]);
        // Virtual Server stuff
        // taken from https://github.com/kubernetes/kubernetes/blob/master/pkg/proxy/ipvs/proxier.go#L295
        Self::exec_or_log_err(&exec, "sysctl", &["-w", "net.ipv4.vs.conntrack=1"]);

        // since we're using conntrac we need to increase the max so we dont exhaust it
        Self::exec_or_log_err(&exec, "sysctl", &["-w", "net.nf_conntrack_max=512000"]);
        Self::exec_or_log_err(&exec, "sysctl", &["-w", "net.ipv4.vs.conn_reuse_mode=0"]);
        Self::exec_or_log_err(&exec, "sysctl", &["-w", "net.ipv4.vs.expire_nodest_conn=1"]);
        Self::exec_or_log_err(
            &exec,
            "sysctl",
            &["-w", "net.ipv4.vs.expire_quiescent_template=1"],
        );

        // These are configurable in kube-proxy, should we allow it??
        // Self::exec_or_log_err(&exec,"sysctl", &["-w", "net.ipv4.all.arp_ignore=1"]);
        // Self::exec_or_log_err(&exec,"sysctl", &["-w", "net.ipv4.all.arp_announce=2"]);

        exec.exec_stdout("sysctl", &["-p"], None)?;
        Ok(())
    }

    fn exec_or_log_err(exec: &Box<dyn ShellExec>, cmd: &str, args: &[&str]) {
        match exec.exec_stdout(cmd, args, None) {
            Ok(_) => (),
            Err(e) => log::error!("{:?}", e),
        }
    }
}
