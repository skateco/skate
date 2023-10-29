use std::error::Error;
use async_ssh2_tokio::client::CommandExecutedResult;
use async_ssh2_tokio::Error as SshError;
use clap::Args;
use strum_macros::EnumString;
use thiserror::Error;
use crate::skate::{HostFileArgs};
use crate::ssh_client::HostInfoResponse;

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: HostFileArgs,
    #[arg(long, long_help = "Url prefix where to find binaries", default_value = "https://skate.on/releases/", env)]
    binary_url_prefix: String,
}

#[derive(Debug, Error, EnumString)]
enum OnError {
    #[error("failed to install skatelet: {0}")]
    InstallationError(String),
}

pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let hosts = crate::skate::read_hosts(args.hosts.hosts_file)?.hosts;

    let results = futures::future::join_all(hosts.into_iter().map(|h| tokio::spawn(async move {
        let c = h.connect().await.unwrap();

        let result = c.get_host_info().await.expect("failed to get host info");
        println!("{:?}", result);
        if result.skatelet_version.is_some() {
            return Ok::<HostInfoResponse, OnError>(result.clone());
        }
        // need to install
        let _ = c.download_skatelet(result.platform).await.expect("failed to download skatelet");

        // double check version
        let result = c.get_host_info().await.expect("failed to get host info");
        if result.skatelet_version.is_some() {
            return Ok::<HostInfoResponse, OnError>(result.clone());
        }

        Err(OnError::InstallationError("skatelet version not found despite installing".to_string()))
    }))).await;

    for result in results {}

    // for mut host in hosts.hosts {
    //     host.connect().await?;
    //
    //     let result = host.execute("hostname;uname -a;").await?;
    //     println!("{}", &result.stdout);
    // }


    // - contact all hosts and check ssh access
    // - upload/download skatelet
    // - run skatelet up
    Ok(())
}
