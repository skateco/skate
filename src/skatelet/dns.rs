use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use fs2::FileExt;
use log::{debug, info, warn, LevelFilter};
use crate::util::NamespacedName;
use std::io::prelude::*;
use syslog::{BasicLogger, Facility, Formatter3164};
use crate::skate::exec_cmd;

#[derive(Debug, Subcommand)]
pub enum Command {
    Add(AddArgs),
    Remove(RemoveArgs),
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
    ip: String,
}

fn retry<T>(retries: u32, f: impl Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    for _ in 0..(retries - 1) {
        let result = f();
        if result.is_ok() {
            return result;
        } else {
            warn!("retrying due to {}", result.err().unwrap());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    f()
}

pub fn add(container_id: String, ip: String) -> Result<(), Box<dyn Error>> {
    ensure_skatelet_dns_conf_dir();

    info!("dns add for {} {}", container_id, ip);

    // TODO - store pod info in store, if no info, break retry loop
    let json = retry(60, || {
        debug!("inspecting container {}",container_id);
        let output = exec_cmd("timeout", &["0.2", "podman", "inspect", container_id.as_str()])?;
        let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
        let pod = json[0]["Pod"].as_str();
        if pod.is_none() {
            warn!("no pod found");
            return Err("no pod found".into());
        }

        debug!("inspecting pod");
        let output = exec_cmd("timeout", &["0.2", "podman", "pod", "inspect", pod.unwrap()])?;
        let pod_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))?;

        let containers: Vec<_> = pod_json["Containers"].as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
            c["Id"].as_str().unwrap()
        ).collect();

        debug!("inspecting all pod containers");
        let args = vec!(vec!("0.2", "podman", "inspect"), containers).concat();

        let output = exec_cmd("timeout", &args)?;
        let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;

        // Check json for [*].State.Health.Status == "healthy"
        let containers: Vec<_> = json.as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
            c["State"]["Health"]["Status"].as_str().unwrap()
        ).collect();

        if containers.into_iter().all(|c| c == "healthy" || c == "") {
            debug!("all containers healthy or no healthcheck");
            return Ok(pod_json);
        };
        return Err("not all containers are healthy".into());
    })?;


    let labels = json["Labels"].as_object().unwrap();
    let ns = labels["skate.io/namespace"].as_str().ok_or_else(|| anyhow!("missing skate.io/namespace label"))?;

    // only add for daemonsets or deployments
    let mut parent_resource = "";

    if labels.contains_key("skate.io/daemonset") {
        parent_resource = "daemonset";
    } else if labels.contains_key("skate.io/deployment") {
        parent_resource = "deployment";
    } else {
        return Ok(())
    }

    let parent_identifer_label = format!("skate.io/{}", parent_resource);

    let app = labels.get(&parent_identifer_label).unwrap().as_str().unwrap();

    let domain = format!("{}.{}.cluster.skate", app, ns);
    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");

    // Do stuff
    lock(Box::new(move || {

        // scope to make sure files closed after
        {
            debug!("updating hosts file");
            // create or open
            let mut addhosts_file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

            writeln!(addhosts_file, "{} {} # {}", ip, domain, container_id).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
        }

        Ok(())
    }))
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    container_id: String,
}

pub fn remove(container_id: String) -> Result<(), Box<dyn Error>> {
    info!("removing dns entry for {}", container_id);
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
        debug!("replacing hosts file");
        fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        Ok(())
    }))
}