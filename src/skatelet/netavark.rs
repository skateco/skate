use std::error::Error;
use std::process::{Command, Stdio};
use netavark::{
    network::types,
    plugin::{Info, Plugin, PluginExec, API_VERSION},
};
use crate::skatelet::dns;

pub fn netavark() {
    // change the version to the version of your plugin
    let info = Info::new("0.1.0".to_owned(), API_VERSION.to_owned(), None);
    PluginExec::new(Exec {}, info).exec();
}

struct Exec {}

impl Plugin for Exec {
    fn create(
        &self,
        network: types::Network,
    ) -> Result<types::Network, Box<dyn std::error::Error>> {
        // your logic here
        Ok(network)
    }

    fn setup(
        &self,
        netns: String,
        opts: types::NetworkPluginExec,
    ) -> Result<types::StatusBlock, Box<dyn std::error::Error>> {
        // add dns entry
        // The fact that we don't have a `?` or `unrwap` here is intentional
        // This disowns the process, which is what we want.
        match opts.network_options.static_ips {
            Some(ips) => {
                // // TODO what if there's multiple ??? I guess find the one on our subnet
                let ip = ips.first().unwrap().to_string();
                let _ = Command::new("skatelet").args(&["dns", "add", &opts.container_id, &ip])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            },
            None => {}

        };
        Ok(types::StatusBlock{
            dns_search_domains: None,
            dns_server_ips: None,
            interfaces: None,
        })
    }

    fn teardown(
        &self,
        netns: String,
        opts: types::NetworkPluginExec,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // remove dns entry
        dns::remove(opts.container_id)?;
        Ok(())
    }
}
