use std::error::Error;
use std::sync::Arc;
use clap::Args;
use crate::skate::{Host, HostFileArgs};
use async_ssh2_tokio::client::{Client, AuthMethod, ServerCheckMethod};
use async_trait::async_trait;

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: HostFileArgs,
}


pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let hosts = crate::skate::read_hosts(args.hosts.hosts_file)?;

    for host in hosts.hosts {
        let auth_method = AuthMethod::with_key_file(&*host.key, None);
        let client = Client::connect(
            (host.host, host.port.unwrap_or(22)),
            &*host.user,
            auth_method,
            ServerCheckMethod::NoCheck,
        ).await?;

        let result = client.execute("echo Hello SSH").await?;
    }


    // - contact all hosts and check ssh access
    // - upload/download skatelet
    // - run skatelet up
    Ok(())
}
