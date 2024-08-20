use std::collections::HashMap;
use std::env::var;
use std::error::Error;
use std::{fs, io};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use cni_plugin::reply::{SuccessReply, VersionReply};
use fs2::FileExt;
use log::{debug, info, error, LevelFilter};
use std::io::prelude::*;
use std::process::{Command, Stdio};
use anyhow::anyhow;
use cni_plugin::config::NetworkConfig;
use serde_json::Value;
use serde_json::Value::String as JsonString;
use syslog::{BasicLogger, Facility, Formatter3164};
use crate::skate::exec_cmd;
use crate::skatelet::dns;
use crate::util::{spawn_orphan_process, NamespacedName};


fn extract_args(config: &NetworkConfig) -> HashMap<String, Value> {
    let env_args = var("CNI_ARGS").and_then(|e| {
        let mut hm = HashMap::new();
        for kv in e.split(";") {
            let mut kv = kv.split("=");
            let k = kv.next().unwrap_or_default();
            let v = kv.next().unwrap_or_default();
            hm.insert(k.to_string(), JsonString(v.to_string()));
        }
        Ok(hm)
    }).unwrap_or_default();

    let mut new_args = config.args.clone();
    new_args.extend(env_args);
    new_args
}

fn prev_result_or_default(config: &NetworkConfig) -> SuccessReply {
    info!("{:?}", var("CNI_ARGS"));
    let prev = extract_prev_result(config.prev_result.clone());
    prev.or(Some(SuccessReply {
        cni_version: config.cni_version.clone(),
        interfaces: Default::default(),
        ips: Default::default(),
        routes: Default::default(),
        dns: Default::default(),
        specific: Default::default(),
    })).unwrap()
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

            if result.ips.len() == 0 {
                return Err("no ips in prev_result".into());
            }


            let container_id = var("CNI_CONTAINERID")?;

            let ip = result.ips[0].address.ip().clone().to_string();

            spawn_orphan_process("skatelet", &["dns", "add", &container_id, &ip]);

            serde_json::to_writer(io::stdout(), &json)?;
        }
        "DEL" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            let container_id = var("CNI_CONTAINERID")?;

            // Do stuff
            dns::remove(container_id)?;
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