use std::env::var;
use std::error::Error;
use std::io;

use cni_plugin::reply::{SuccessReply, VersionReply};
use log::{info, error};
use anyhow::anyhow;
use cni_plugin::config::NetworkConfig;
use serde_json::Value;
use crate::skatelet::dns;
use crate::skatelet::dns::RemoveArgs;
use crate::util::spawn_orphan_process;


fn prev_result_or_default(config: &NetworkConfig) -> SuccessReply {
    info!("{:?}", var("CNI_ARGS"));
    let prev = extract_prev_result(config.prev_result.clone());
    prev.unwrap_or(SuccessReply {
        cni_version: config.cni_version.clone(),
        interfaces: Default::default(),
        ips: Default::default(),
        routes: Default::default(),
        dns: Default::default(),
        specific: Default::default(),
    })
}

fn extract_prev_result(prev_value: Option<Value>) -> Option<SuccessReply> {
    prev_value.and_then(|prev_value| {
        let prev_result: Result<SuccessReply, _> = serde_json::from_value(prev_value);
        match prev_result {
            Ok(prev_result) => {
                Some(prev_result)
            }
            Err(e) => {
                error!("unable to parse prev_result: {}", e);
                None
            }
        }
    })
}


pub fn cni() {

    match run() {
        Ok(_) => {},
        Err(e) => {
            panic!("{}", e)
        }
    }
}

fn run() -> Result<String, Box<dyn Error>> {

    let cmd = var("CNI_COMMAND").unwrap_or_default();
    match cmd.as_str() {
        "ADD" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            let config: NetworkConfig = serde_json::from_value(json.clone()).map_err(|e| anyhow!("failed to parse config: {}", e))?;

            let result = prev_result_or_default(&config);

            if result.ips.is_empty() {
                return Err("no ips in prev_result".into());
            }


            let container_id = var("CNI_CONTAINERID")?;

            let ip = result.ips[0].address.ip().clone().to_string();

            spawn_orphan_process("skatelet", ["dns", "add", &container_id, &ip]);

            serde_json::to_writer(io::stdout(), &json)?;
        }
        "DEL" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            let container_id = var("CNI_CONTAINERID")?;

            // Do stuff
            dns::remove(RemoveArgs{container_id: Some(container_id), pod_id: None})?;
            serde_json::to_writer(io::stdout(), &json)?;
        }
        "CHECK" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;
            if !json["prevResult"].is_object() {
                return Err(anyhow!("failed to parse prevResult").into());
            }
            let prev_result = json["prevResult"].clone();
            let response = serde_json::to_string(&prev_result).map_err(|e| anyhow!("failed to serialize prev result: {}", e))?;
            serde_json::to_writer(io::stdout(), &response).map_err(|e| anyhow!("failed to serialize version response: {}", e))?;
        }
        "VERSION" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            // TODO get version from json

            let cni_version = json["cniVersion"].as_str().unwrap_or("0.4.0");

            let response = VersionReply {
                cni_version: cni_version.parse()?,
                supported_versions: vec!["0.4.0".parse()?],
            };

            serde_json::to_writer(io::stdout(), &response).map_err(|e| anyhow!("failed to serialize version response: {}", e))?;
        }
        _ => {
            return Err("unknown command".into());
        }
    };
    Ok(cmd)
}