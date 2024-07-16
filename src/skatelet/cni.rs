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
use crate::skate::exec_cmd;


fn conf_path_str() -> String {
    "/var/lib/skate/cni".to_string()
}

fn lock<T>(network_name: &str, cb: &dyn Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(&conf_path_str()).join(network_name).join("lock");
    let lock_file = File::create(lock_path.clone()).map_err(|e| anyhow!("failed to create/open lock file: {}", e))?;
    debug!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    debug!("locked {}", lock_path.display());

    let result = cb();

    lock_file.unlock()?;

    result
}

fn ensure_skatelet_cni_conf_dir(dir_name: &str) {
    let conf_str = conf_path_str();
    let conf_path = Path::new(&conf_str);
    let net_path = conf_path.join(dir_name);

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

fn write_last_run_file(msg: &str) {
    let last_err_path = Path::new(&conf_path_str()).join(".last_run.log");
    let mut last_err_file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(last_err_path).unwrap();
    writeln!(last_err_file, "{}", msg).unwrap();
}

pub fn cni() {
    match run() {
        Ok(warning) => {
            write_last_run_file(&format!("WARNING: {}", warning))
        }
        Err(e) => {
            write_last_run_file(&format!("ERROR: {}", e));
            panic!("{}", e)
        }
    }
}

fn run() -> Result<String, Box<dyn Error>> {
    let conf_str = conf_path_str();
    let conf_path = Path::new(&conf_str);

    fs::create_dir_all(conf_path).unwrap();

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

            // get podman info from sqlitedb in /var/lib/containers/storage/db.sql
            let pod_json = exec_cmd(
                "sqlite3",
                &[
                    "/var/lib/containers/storage/db.sql",
                    &format!("select p.json from ContainerConfig c join PodConfig p on c.PodID = p.id where c.id = '{}'", container_id)
                ],
            )?;

            if pod_json.is_empty() {
                serde_json::to_writer(io::stdout(), &json)?;
                return Ok("ADD: not a pod".to_string());
            }

            let pod_value: Value = serde_json::from_str(&pod_json).map_err(|e| anyhow!("failed to parse pod json for {} from {}: {}", container_id, pod_json, e))?;

            let labels = pod_value["labels"].as_object().unwrap_or(&serde_json::Map::new()).clone();

            if !labels.contains_key("app") || !labels.contains_key("skate.io/namespace") {
                serde_json::to_writer(io::stdout(), &json)?;
                return Ok("ADD: missing labels".to_string());
            }

            // domain is <app>.<skate.io/namespace>.cluster.skate
            let app = labels.get("app").ok_or(anyhow!("missing label"))?.as_str().unwrap_or_default().to_string();
            let ns = labels.get("skate.io/namespace").ok_or(anyhow!("missing label"))?.as_str().unwrap_or_default().to_string();
            if ns == "" {
                serde_json::to_writer(io::stdout(), &json)?;
                return Ok("ADD: namespace empty".to_string());
            }
            let domain = format!("{}.{}.cluster.skate", app.clone(), ns.clone());
            let ip = result.ips[0].address.ip().to_string();

            ensure_skatelet_cni_conf_dir(&config.name);

            // Do stuff
            lock(&config.name, &|| {
                let addnhosts_path = Path::new(&conf_path_str()).join(config.name.clone()).join("addnhosts");

                // scope to make sure files closed after
                {

                    // create or open
                    let mut addhosts_file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .append(true)
                        .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

                    writeln!(addhosts_file, "{} {} # {}", ip, domain, container_id).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
                }

                Ok(())
            })?;

            serde_json::to_writer(io::stdout(), &json)?;
        }
        "DEL" => {
            let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!("failed to parse stdin: {}", e))?;

            let config: NetworkConfig = serde_json::from_value(json.clone()).map_err(|e| anyhow!("failed to parse config: {}", e))?;

            let container_id = var("CNI_CONTAINERID")?;

            // Do stuff
            ensure_skatelet_cni_conf_dir(&config.name);
            lock(&config.name, &|| {
                let _args = extract_args(&config);

                let addnhosts_path = Path::new(&conf_path_str()).join(config.name.clone()).join("addnhosts");
                let newaddnhosts_path = Path::new(&conf_path_str()).join(config.name.clone()).join("addnhosts-new");

                // scope to make sure files closed after
                {
                    // create or open

                    let addhosts_file = OpenOptions::new()
                        .read(true)
                        .open(addnhosts_path.clone());

                    if addhosts_file.is_err() {
                        return Ok(());
                    }
                    let addhosts_file = addhosts_file?;

                    let newaddhosts_file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(newaddnhosts_path.clone())?;

                    let reader = BufReader::new(&addhosts_file);
                    let mut writer = BufWriter::new(&newaddhosts_file);

                    for (_index, line) in reader.lines().enumerate() {
                        let line = line?;
                        if !line.ends_with(&container_id) {
                            writeln!(writer, "{}", line)?;
                        }
                    }
                }
                fs::rename(&newaddnhosts_path, &addnhosts_path)?;

                serde_json::to_writer(io::stdout(), &json)?;
                Ok(())
            })?;
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