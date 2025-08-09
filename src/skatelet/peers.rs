use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::skatelet::database::peer::{delete_peers, list_peers, upsert_peer, Peer};
use anyhow::anyhow;
use clap::{Args, Subcommand};
use itertools::Itertools;
use sqlx::Acquire;
use strum_macros::IntoStaticStr;
use validator::Validate;

pub trait PeersDeps: With<dyn ShellExec> + WithDB {}

pub struct Peers<D: PeersDeps> {
    pub deps: D,
}

#[derive(Clone, Debug, Args)]
pub struct PeersArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Debug, Args)]
pub struct SetPeersArgs {
    #[arg(
        long,
        long_help = "The peer to add. Format is colon separated. Example `<node_name>:<peer_host>:<subnet_cidr>`."
    )]
    peer: Vec<String>,
}

#[derive(Clone, Debug, Subcommand, IntoStaticStr)]
enum Commands {
    Set(SetPeersArgs),
    List,
}

impl<D: PeersDeps> Peers<D> {
    pub async fn peers(&self, args: PeersArgs) -> Result<(), SkateError> {
        match args.command {
            Commands::List => self.list_peers().await,
            Commands::Set(args) => self.set_peers(args).await,
        }
    }

    async fn list_peers(&self) -> Result<(), SkateError> {
        let db = self.deps.get_db();
        let peers = list_peers(&db).await?;
        println!("{:?}", peers);
        Ok(())
    }

    async fn set_peers(&self, args: SetPeersArgs) -> Result<(), SkateError> {
        let mut peers = vec![];
        for peer in args.peer {
            peers.push(Self::arg_to_peer(&peer)?)
        }
        let db = self.deps.get_db();
        let mut tx = db.begin().await?;

        delete_peers(tx.acquire().await?).await?;
        for peer in peers {
            upsert_peer(tx.acquire().await?, &peer).await?;
        }

        tx.commit().await?;
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

        // delete all peers
        // insert all peers

        peer.validate()?;

        Ok(peer)
    }
}
