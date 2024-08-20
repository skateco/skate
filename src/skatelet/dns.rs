use std::error::Error;
use std::{fs, process};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use fs2::FileExt;
use log::{debug, info, warn, LevelFilter};
use crate::util::{spawn_orphan_process, NamespacedName};
use std::io::prelude::*;
use std::process::Stdio;
use serde_json::Value;
use syslog::{BasicLogger, Facility, Formatter3164};
use crate::skate::exec_cmd;

#[derive(Debug, Subcommand)]
pub enum Command {
    Add(AddArgs),
    Remove(RemoveArgs),
    Enable(EnableArgs),
}
#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    command: Command,
}

pub fn dns(args: DnsArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        Command::Add(add_args) => add(add_args.container_id, add_args.ip),
        Command::Remove(remove_args) => remove(remove_args.container_id),
        Command::Enable(enable_args) => wait_and_enable_healthy(enable_args.container_id),
    }
}

fn conf_path_str() -> String {
    "/var/lib/skate/dns".to_string()
}

fn lock<T>(cb: Box<dyn FnOnce() -> Result<T, Box<dyn Error>>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(&conf_path_str()).join("lock");
    let lock_file = File::create(lock_path.clone()).map_err(|e| anyhow!("failed to create/open lock file: {}", e))?;
    info!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    info!("locked {}", lock_path.display());

    let result = cb();

    lock_file.unlock()?;
    info!("unlocked {}", lock_path.display());

    result
}

fn ensure_skatelet_dns_conf_dir() {
    let conf_str = conf_path_str();
    let conf_path = Path::new(&conf_str);

    fs::create_dir_all(conf_path).unwrap();
}


#[derive(Debug, Args)]
pub struct AddArgs {
    container_id: String,
    ip: Option<String>,
}

fn retry<T>(retries: u32, f: impl Fn() -> Result<T, (bool, Box<dyn Error>)>) -> Result<T, Box<dyn Error>> {
    for _ in 0..(retries - 1) {
        let result = f();
        match result {
            Ok(ok) => return Ok(ok),
            Err((cont, err)) => {
                if !cont {
                    return Err(err);
                }

                warn!("retrying due to {}", err)
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    match f() {
        Ok(ok) => Ok(ok),
        Err((_, err)) => Err(err)
    }
}

pub fn add(container_id: String, supplied_ip: Option<String>) -> Result<(), Box<dyn Error>> {
    ensure_skatelet_dns_conf_dir();
    let log_tag = format!("{}::add", container_id);

    info!("{} dns add for {} {:?}", log_tag, container_id, supplied_ip);

    // TODO - store pod info in store, if no info, break retry loop
    let (extracted_ip, json) = retry(10, || {
        debug!("{} inspecting container {}",log_tag, container_id);
        let output = exec_cmd("timeout", &["0.2", "podman", "inspect", container_id.as_str()]).map_err(|e| (true, e.into()))?;
        let container_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e)).map_err(|e| (false, e.into()))?;
        let is_infra = container_json[0]["IsInfra"].as_bool().unwrap();
        if !is_infra {
            warn!("{} not infra container", log_tag);
            return Err((false, "not infra container".into()));
        }

        let ip = extract_skate_ip(container_json[0].clone());

        let pod = container_json[0]["Pod"].as_str();
        if pod.is_none() {
            warn!("{} no pod found", log_tag);
            return Err((false, "no pod found".into()));
        }


        debug!("{} inspecting pod", log_tag);
        let output = exec_cmd("timeout", &["0.2", "podman", "pod", "inspect", pod.unwrap()]).map_err(|e| (true, e.into()))?;
        let pod_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e)).map_err(|e| (false, e.into()))?;
        Ok((ip, pod_json))
    })?;

    let ip = match supplied_ip {
        Some(ip) => Some(ip),
        None => extracted_ip
    };

    if ip.is_none() {
        warn!("{} no ip supplied or found for network 'skate'", log_tag);
        return Ok(());
    }
    let ip = ip.unwrap();


    let labels = json["Labels"].as_object().unwrap();
    let ns = labels["skate.io/namespace"].as_str().ok_or_else(|| anyhow!("missing skate.io/namespace label"))?;

    // only add for daemonsets or deployments
    let mut parent_resource = "";

    if labels.contains_key("skate.io/daemonset") {
        parent_resource = "daemonset";
    } else if labels.contains_key("skate.io/deployment") {
        parent_resource = "deployment";
    } else {
        info!("not a daemonset or deployment, skipping");
        return Ok(())
    }

    let parent_identifer_label = format!("skate.io/{}", parent_resource);

    let app = labels.get(&parent_identifer_label).unwrap().as_str().unwrap();

    let domain = format!("{}.{}.cluster.skate", app, ns);
    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");

    let container_id_cpy = container_id.clone();
    // Do stuff
    let result = lock(Box::new(move || {

        // scope to make sure files closed after
        {
            debug!("{} updating hosts file", log_tag);
            // create or open
            let mut addhosts_file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

            // write with comment for now
            writeln!(addhosts_file, "#{} {} # {}", ip, domain, container_id_cpy).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
        }

        Ok(())
    }));

    if result.is_ok() {
        spawn_orphan_process("skatelet", &["dns", "enable", &container_id]);
    }
    result
}

#[derive(Debug, Args)]
pub struct EnableArgs {
    container_id: String,
}

fn extract_skate_ip(json: Value) -> Option<String> {
    json["NetworkSettings"]["Networks"].as_object().unwrap().iter().filter_map(|(k, v)| {
        if k.eq("skate") {
            match v["IPAddress"].as_str() {
                Some(ip) => match ip {
                    "" => None,
                    _ => Some(ip.to_string())
                }
                None => None
            }
        } else {
            None
        }
    }).collect::<Vec<String>>().first().and_then(|s| Some(s.clone()))
}

pub fn wait_and_enable_healthy(container_id: String) -> Result<(), Box<dyn Error>> {
    let log_tag = format!("{}::enable", container_id);
    debug!("{} inspecting container {}",log_tag, container_id);
    let output = exec_cmd("timeout", &["0.2", "podman", "inspect", container_id.as_str()])?;
    let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
    let pod = json[0]["Pod"].as_str();
    if pod.is_none() {
        warn!("{} no pod found", log_tag);
        return Err("no pod found".into());
    }

    debug!("{} inspecting pod", log_tag);
    let output = exec_cmd("timeout", &["0.2", "podman", "pod", "inspect", pod.unwrap()])?;
    let pod_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))?;

    let containers: Vec<_> = pod_json["Containers"].as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
        c["Id"].as_str().unwrap()
    ).collect();

    let args = vec!(vec!("0.2", "podman", "inspect"), containers).concat();

    let mut healthy = false;
    for _ in 0..60 {
        debug!("{} inspecting all pod containers",log_tag);
        let output = exec_cmd("timeout", &args)?;
        let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;

        // Check json for [*].State.Health.Status == "healthy"
        let containers: Vec<_> = json.as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
            c["State"]["Health"]["Status"].as_str().unwrap()
        ).collect();

        if containers.iter().any(|c| c.clone() == "unhealthy") {
            debug!("{} at least one container unhealthy",log_tag);
            // do nothing
            return Ok(());
        };

        if containers.into_iter().all(|c| c == "healthy" || c == "") {
            debug!("{} all containers healthy or no healthcheck",log_tag);
            healthy = true;
            break;
        };
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    if !healthy {
        warn!("{} timed out waiting for all containers to be healthy",log_tag);
        return Ok(());
    }

    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");
    let newaddnhosts_path = Path::new(&conf_path_str()).join("addnhosts-new");

    lock(Box::new(move || {
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
                if line.ends_with(&container_id) {
                    debug!("{} enabling dns entry for {}", log_tag,container_id);
                    writeln!(writer, "{}", line.trim().trim_start_matches('#'))?;
                } else {
                    writeln!(writer, "{}", line)?;
                }
            }
        }
        debug!("{} replacing hosts file",log_tag);
        fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        Ok(())
    }))
}


#[derive(Debug, Args)]
pub struct RemoveArgs {
    container_id: String,
}

pub fn remove(container_id: String) -> Result<(), Box<dyn Error>> {
    let log_tag = format!("{}::remove", container_id);
    info!("{} removing dns entry for {}", log_tag, container_id);
    ensure_skatelet_dns_conf_dir();
    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");
    let newaddnhosts_path = Path::new(&conf_path_str()).join("addnhosts-new");
    // Do stuff
    lock(Box::new(move || {
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
        debug!("{} replacing hosts file", log_tag);
        fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        Ok(())
    }))
}