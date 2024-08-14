use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use fs2::FileExt;
use log::debug;
use crate::util::NamespacedName;
use std::io::prelude::*;
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
    debug!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    debug!("locked {}", lock_path.display());

    let result = cb();

    lock_file.unlock()?;

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

fn retry<T>(attempts: u32, f: impl Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    for _ in 0..(attempts - 1) {
        let result = f();
        if result.is_ok() {
            return result;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    f()
}

pub fn add(container_id: String, ip: String) -> Result<(), Box<dyn Error>> {
    ensure_skatelet_dns_conf_dir();

    // Do stuff
    lock(Box::new(move || {

        // TODO - store pod info in store, if no info, break retry loop
        let json = retry(60, || {
            let output = exec_cmd("podman", &["inspect", container_id.as_str()])?;
            let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
            let pod = json[0]["Pod"].as_str();
            if pod.is_none() {
                return Err("no pod found".into());
            }

            let output = exec_cmd("podman", &["pod", "inspect", pod.unwrap()])?;
            let pod_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))?;

            let mut containers: Vec<_> = pod_json["Containers"].as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
                c["Id"].as_str().unwrap()
            ).collect();

            let args = vec!(vec!("inspect"), containers).concat();

            let output = exec_cmd("podman", &args)?;
            let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;

            // Check json for [*].State.Health.Status == "healthy"
            let containers: Vec<_> = json.as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
                c["State"]["Health"]["Status"].as_str().unwrap()
            ).collect();

            if containers.into_iter().all(|c| c == "healthy" || c == "") {
                return Ok(pod_json)
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
    }))
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    container_id: String,
}

pub fn remove(container_id: String) -> Result<(), Box<dyn Error>> {
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
        fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        Ok(())
    }))
}