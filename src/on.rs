use std::error::Error;
use anyhow::anyhow;
use itertools::{Either, Itertools};
use clap::Args;
use strum_macros::EnumString;
use thiserror::Error;
use crate::config::Node;
use crate::skate::{ConfigFileArgs};
use crate::ssh_client::HostInfoResponse;

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: ConfigFileArgs,
    #[arg(long, long_help = "Url prefix where to find binaries", default_value = "https://skate.on/releases/", env)]
    binary_url_prefix: String,
}


async fn install_skatelet(h: &Node) -> Result<HostInfoResponse, Box<dyn Error + Send>> {
    let c = match h.connect().await {
        Ok(c) => c,
        Err(err) => {
            return Err(anyhow!("failed to connect").context(err).into());
        }
    };

    let result = c.get_host_info().await.expect("failed to get host info");
    if result.skatelet_version.is_some() {
        return Ok(result.clone());
    }
    // need to install
    let _ = c.download_skatelet(result.platform).await.expect("failed to download skatelet");

    // double check version
    let result = c.get_host_info().await.expect("failed to get host info");
    if result.skatelet_version.is_some() {
        return Ok(result.clone());
    }

    Err(anyhow!("skatelet version not found despite installing").into())
}

pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let config = crate::skate::read_config(args.hosts.skateconfig)?;
    let cluster = config.current_cluster()?;
    let hosts = cluster.nodes;

    let resolved_hosts = hosts.into_iter().map(|h| Node {
        host: h.host,
        port: h.port.or(Some(22)),
        user: h.user.or(cluster.default_user.clone()),
        key: h.key.or(cluster.default_key.clone()),
    });

    let results = futures::future::join_all(resolved_hosts.into_iter().map(|h| tokio::spawn(async move {
        install_skatelet(&h).await
    }))).await;

    let (success, failed): (Vec<HostInfoResponse>, Vec<String>) = results.into_iter().partition_map(|v| match v {
        Ok(v) => match v {
            Ok(v) => Either::Left(v),
            Err(v) => Either::Right(v.to_string())
        }
        Err(v) => Either::Right(v.to_string())
    });

    for info in success {
        println!("✅  {} ({:?} - {}) running skatelet version {}",
                 info.hostname,
                 info.platform.os,
                 info.platform.arch,
                 info.skatelet_version.unwrap_or_default().split_whitespace().last().unwrap_or_default())
    }

    if failed.len() > 0 {
        eprintln!();
        return Err(anyhow!("\n".to_string()+&failed.join("\n")).into());
    }


    // - contact all hosts and check ssh access
    // - upload/download skatelet
    // - run skatelet up
    Ok(())
}
