use std::collections::HashMap;
use std::env::var;
use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions, read_to_string};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use cni_plugin::{Cni, logger};
use cni_plugin::reply::{ErrorReply, reply, SuccessReply};
use fs2::FileExt;
use log::{debug, info, warn, error};
use std::io::prelude::*;
use anyhow::anyhow;
use cni_plugin::config::NetworkConfig;
use serde_json::Value;
use serde_json::Value::String as JsonString;
use crate::skate::exec_cmd;
use crate::skatelet::skatelet::VAR_PATH;

fn conf_path() -> String {
    "/var/lib/skatelet/cni".to_string()
}

fn lock<T>(network_name: &str, cb: &dyn Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(&conf_path()).join(network_name.clone()).join("lock");
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
    let net_path = conf_path.join(net_name.clone());

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
    logger::install("skatelet.log");

    match Cni::load() {
        Cni::Add { container_id, ifname, netns, path, config } => {
            ensure_paths(&config.name);
            info!("{:?}", config);

            let mut result = prev_result_or_default(&config);
            let args = extract_args(&config);

            match lock(&config.name, &|| {

                // read file at conf_path()/<interface>/addnhosts
                let addnhosts_path = Path::new(&conf_path()).join(config.name.clone()).join("addnhosts");
                // create or open
                let mut addhosts_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(true)
                    .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

                let mut names = config.runtime.as_ref().and_then(|r| Some(r.aliases.clone())).unwrap_or_default();

                match args.get("K8S_POD_NAME") {
                    Some(JsonString(pod_name)) => names.push(pod_name.clone()),
                    _ => {}
                }


                if result.ips.len() == 0 {
                    return Err("no ips in prev_result".into());
                }

                let ip_str = result.ips[0].address.ip().to_string();

                for name in names {
                    writeln!(addhosts_file, "{} {}", ip_str, name).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
                }
                Ok(())
            }) {
                Err(e) => {
                    reply(ErrorReply {
                        cni_version: config.cni_version,
                        code: 1, // TODO
                        msg: &e.to_string(),
                        details: "".to_string(),
                    })
                }
                Ok(()) => {}
            }
            reply(result);
        }
        Cni::Del { container_id, ifname, netns, path, config } => {
            ensure_paths(&config.name);
            match lock(&config.name, &|| {
                let mut result = prev_result_or_default(&config);
                let args = extract_args(&config);

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

                    for (index, line) in reader.lines().enumerate() {
                        let line = line.as_ref().unwrap();
                        if !line.starts_with(&ip) {
                            writeln!(writer, "{}", line)?;
                        }
                    }
                }
                fs::rename(&newaddnhosts_path, &addnhosts_path)?;
                Ok(())
            }) {
                Err(e) => {
                    reply(ErrorReply {
                        cni_version: config.cni_version,
                        code: 1, // TODO
                        msg: &e.to_string(),
                        details: "".to_string(),
                    })
                }
                Ok(()) => {}
            }
            reply(prev_result_or_default(&config))
        }
        Cni::Check { container_id, ifname, netns, path, config } => {
            ensure_paths(&config.name);

            let prev_result = prev_result_or_default(&config);
            reply(prev_result);
        }
        Cni::Version(_) => {
            eprintln!("version");
        }
    }
}