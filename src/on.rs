use std::error::Error;
use async_ssh2_tokio::client::CommandExecutedResult;
use async_ssh2_tokio::Error as SshError;
use clap::Args;
use crate::skate::{HostFileArgs};

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: HostFileArgs,
}


pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let hosts = crate::skate::read_hosts(args.hosts.hosts_file)?.hosts;

    let results = futures::future::join_all(hosts.into_iter().map(|h| tokio::spawn(async move {
        let c = h.connect().await.unwrap();
        let result = c.execute("hostname && uname -a;").await.unwrap();
        println!("{}", result.stdout.clone());
        Ok::<CommandExecutedResult, SshError>(result)
    }))).await;

    for result in results {

    }

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
