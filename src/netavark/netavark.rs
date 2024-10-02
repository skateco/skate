#![cfg(target_os = "linux")]

use crate::skatelet::dns;
use crate::skatelet::dns::RemoveArgs;
use crate::util::spawn_orphan_process;
use log::info;
use netavark::{
    network::types,
    plugin::{Info, Plugin, PluginExec, API_VERSION},
};

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
        info!("create");
        // your logic here
        Ok(network)
    }

    fn setup(
        &self,
        _netns: String,
        opts: types::NetworkPluginExec,
    ) -> Result<types::StatusBlock, Box<dyn std::error::Error>> {
        info!("setup");
        // add dns entry
        // The fact that we don't have a `?` or `unrwap` here is intentional
        // This disowns the process, which is what we want.
        if let Some(ips) = opts.network_options.static_ips {
            // TODO what if there's multiple ??? I guess find the one on our subnet
            let ip = ips.first().unwrap().to_string();
            spawn_orphan_process("skatelet", ["dns", "add", &opts.container_id, &ip]);
        };
        Ok(types::StatusBlock {
            dns_search_domains: None,
            dns_server_ips: None,
            interfaces: None,
        })
    }

    fn teardown(
        &self,
        _netns: String,
        opts: types::NetworkPluginExec,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("teardown");
        // remove dns entry
        dns::remove(RemoveArgs{container_id: Some(opts.container_id), pod_id: None})?;
        Ok(())
    }
}
