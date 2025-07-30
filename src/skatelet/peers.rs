use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::skatelet::database::peer::{list_peers, Peer};
use anyhow::anyhow;
use clap::Args;
use itertools::Itertools;
use std::net::ToSocketAddrs;
use validator::Validate;

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
        let mut peers = vec![];
        for peer in args.peer {
            peers.push(Self::arg_to_peer(&peer)?)
        }
        Ok(())
    }

    fn arg_to_peer(arg: &str) -> Result<Peer, SkateError> {
        let parts = arg.split(":").collect_vec();
        if parts.len() != 3 {
            return Err(anyhow!("--peer argument should contain 3 components").into());
        }

        let peer = Peer {
            id: 0,
            node_name: parts[0].to_string(),
            host: parts[1].to_string(),
            subnet_cidr: parts[2].to_string(),
            created_at: Default::default(),
            updated_at: Default::default(),
        };

        peer.validate()?;

        Ok(peer)
    }
}
