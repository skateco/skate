use std::error::Error;
use clap::Args;
use crate::skate::{HostFileArgs};

#[derive(Debug, Args)]
pub struct OnArgs {
    #[command(flatten)]
    hosts: HostFileArgs,
}


pub async fn on(args: OnArgs) -> Result<(), Box<dyn Error>> {
    let hosts = crate::skate::read_hosts(args.hosts.hosts_file)?;

    for mut host in hosts.hosts {
        host.connect().await?;

        let result = host.execute(format!("echo Hello")).await?;
        println!("{}", &result.stdout);
    }


    // - contact all hosts and check ssh access
    // - upload/download skatelet
    // - run skatelet up
    Ok(())
}
