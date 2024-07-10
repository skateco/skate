use std::collections::HashMap;
use std::env::var;
use std::error::Error;
use std::{fs, io};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use cni_plugin::reply::{SuccessReply, VersionReply};
use fs2::FileExt;
use log::{debug, info, error};
use std::io::prelude::*;
use anyhow::anyhow;
use cni_plugin::config::NetworkConfig;
use serde_json::Value;
use serde_json::Value::String as JsonString;



fn conf_path() -> String {
    "/var/lib/skatelet/cni".to_string()
}

fn lock<T>(network_name: &str, cb: &dyn Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(&conf_path()).join(network_name).join("lock");
    let lock_file = File::create(lock_path.clone()).map_err(|e| anyhow!("failed to create/open lock file: {}", e))?;
    debug!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    debug!("locked {}", lock_path.display());

    let result = cb();

    lock_file.unlock()?;

    result
}

fn ensure_paths(net_name: &str) {
    let conf_path_str = conf_path();
    let conf_path = Path::new(&conf_path_str);
    let net_path = conf_path.join(net_name);

    fs::create_dir_all(conf_path).unwrap();
    fs::create_dir_all(net_path).unwrap();
}

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
        Ok(_) => {}
        Err(_e) => {
            // handle error formatting
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cmd = var("CNI_COMMAND").unwrap_or_default();
    match cmd.as_str() {
        "ADD" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;
            if !json["prevResult"].is_object() {
                return Err(anyhow!("failed to parse prevResult").into());
            }
            let prev_result = json["prevResult"].clone();
            let output = serde_json::to_string(&prev_result).map_err(|e| anyhow!("failed to serialize prev result: {}", e))?;
            print!("{}", output);
        }
        "DEL" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;
            if !json["prevResult"].is_object() {
                return Err(anyhow!("failed to parse prevResult").into());
            }

            let prev_result = json["prevResult"].clone();
            let output = serde_json::to_string(&prev_result).map_err(|e| anyhow!("failed to serialize prev result: {}", e))?;

            let config: NetworkConfig = serde_json::from_value(json.clone()).map_err(|e| anyhow!("failed to parse config: {}", e))?;
            // Do stuff
            ensure_paths(&config.name);
            lock(&config.name, &|| {
                let result = prev_result_or_default(&config);
                let _args = extract_args(&config);

                let addnhosts_path = Path::new(&conf_path()).join(config.name.clone()).join("addnhosts");
                let newaddnhosts_path = Path::new(&conf_path()).join(config.name.clone()).join("addnhosts-new");

                // scope to make sure files closed after
                {
                    // create or open

                    let addhosts_file = OpenOptions::new()
                        .read(true)
                        .open(addnhosts_path.clone())
                        .unwrap();

                    let newaddhosts_file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(newaddnhosts_path.clone())
                        .unwrap();

                    let reader = BufReader::new(&addhosts_file);
                    let mut writer = BufWriter::new(&newaddhosts_file);


                    if result.ips.len() == 0 {
                        return Err("no ips in prev_result".into());
                    }

                    let ip = result.ips[0].address.ip().to_string();

                    for (_index, line) in reader.lines().enumerate() {
                        let line = line.as_ref().unwrap();
                        if !line.starts_with(&ip) {
                            writeln!(writer, "{}", line)?;
                        }
                    }
                }
                fs::rename(&newaddnhosts_path, &addnhosts_path)?;

                println!("{}", output);
                Ok(())
            })?;
        }
        "CHECK" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;
            if !json["prevResult"].is_object() {
                return Err(anyhow!("failed to parse prevResult").into());
            }
            let prev_result = json["prevResult"].clone();
            let output = serde_json::to_string(&prev_result).map_err(|e| anyhow!("failed to serialize prev result: {}", e))?;
            print!("{}", output);
        }
        "VERSION" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            // TODO get version from json

            let cni_version = json["cniVersion"].as_str().unwrap_or("0.4.0");

            let response = VersionReply {
                cni_version: cni_version.parse()?,
                supported_versions: vec!["0.4.0".parse().unwrap()],
            };

            serde_json::to_writer(io::stdout(), &response).map_err(|e| anyhow!("failed to serialize version response: {}", e))?;
        }
        _ => {
            eprintln!("unknown command: {}", cmd);
        }


        // match Cni::load() {
        //     Cni::Del { container_id, ifname, netns, path, config } => {
        //         ensure_paths(&config.name);
        //         match lock(&config.name, &|| {
        //             let mut result = prev_result_or_default(&config);
        //             let args = extract_args(&config);
        //
        //             let addnhosts_path = Path::new(&conf_path()).join(config.name.clone()).join("addnhosts");
        //             let newaddnhosts_path = Path::new(&conf_path()).join(config.name.clone()).join("addnhosts-new");
        //
        //             // scope to make sure files closed after
        //             {
        //                 // create or open
        //
        //                 let addhosts_file = OpenOptions::new()
        //                     .read(true)
        //                     .open(addnhosts_path.clone())
        //                     .unwrap();
        //
        //                 let newaddhosts_file = OpenOptions::new()
        //                     .create(true)
        //                     .write(true)
        //                     .truncate(true)
        //                     .open(newaddnhosts_path.clone())
        //                     .unwrap();
        //
        //                 let reader = BufReader::new(&addhosts_file);
        //                 let mut writer = BufWriter::new(&newaddhosts_file);
        //
        //
        //                 if result.ips.len() == 0 {
        //                     return Err("no ips in prev_result".into());
        //                 }
        //
        //                 let ip = result.ips[0].address.ip().to_string();
        //
        //                 for (index, line) in reader.lines().enumerate() {
        //                     let line = line.as_ref().unwrap();
        //                     if !line.starts_with(&ip) {
        //                         writeln!(writer, "{}", line)?;
        //                     }
        //                 }
        //             }
        //             fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        //             Ok(())
        //         }) {
        //             Err(e) => {
        //                 reply(ErrorReply {
        //                     cni_version: config.cni_version,
        //                     code: 1, // TODO
        //                     msg: &e.to_string(),
        //                     details: "".to_string(),
        //                 })
        //             }
        //             Ok(()) => {}
        //         }
        //         reply(prev_result_or_default(&config))
        //     }
        //     Cni::Check { container_id, ifname, netns, path, config } => {
        //         ensure_paths(&config.name);
        //
        //         let prev_result = prev_result_or_default(&config);
        //         reply(prev_result);
        //     }
        //     Cni::Version(_) => {
        //         eprintln!("version");
        //     }
        // }
    };
    Ok(())
}