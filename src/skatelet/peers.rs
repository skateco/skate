use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::skatelet::database::peer::list_peers;
use clap::Args;
use std::net::ToSocketAddrs;

pub trait PeersDeps: With<dyn ShellExec> + WithDB {}

pub struct Peers<D: PeersDeps> {
    pub deps: D,
}

#[derive(Clone, Debug, Args)]
pub struct PeersArgs {
    #[arg(
        long,
        long_help = "The peer to add. Format is colon separated. Example `<node_name>:<peer_host>:<subnet_cidr>`."
    )]
    peer: Vec<String>,
}

impl<D: PeersDeps> Peers<D> {
    pub async fn peers(&self, args: PeersArgs) -> Result<(), SkateError> {
        Ok(())
    }
}
