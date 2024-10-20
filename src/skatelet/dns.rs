use std::error::Error;
use std::{fs, panic};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use log::{debug, info, warn};
use crate::util::{lock_file, spawn_orphan_process};
use std::io::prelude::*;
use serde_json::Value;
use crate::errors::SkateError;
use crate::skate::exec_cmd;
use crate::skatelet::skatelet::log_panic;

#[derive(Debug, Subcommand)]
pub enum Command {
    Add(AddArgs),
    Remove(RemoveArgs),
    Enable(EnableArgs),
    Reload,
}

#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    command: Command,
}

pub fn dns(args: DnsArgs) -> Result<(), SkateError> {
    panic::set_hook(Box::new(move |info| {
        log_panic(info)
    }));
    match args.command {
        Command::Add(add_args) => add(add_args.container_id, add_args.ip),
        Command::Remove(remove_args) => remove(remove_args),
        Command::Enable(enable_args) => wait_and_enable_healthy(enable_args.container_id),
        Command::Reload => reload()
    }
}

fn conf_path_str() -> String {
    "/var/lib/skate/dns".to_string()
}

fn lock<T>(cb: Box<dyn FnOnce() -> Result<T, Box<dyn Error>>>) -> Result<T, SkateError> {
    let result=  lock_file(&format!("{}/lock", conf_path_str()), cb)?;
    Ok(result)
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

pub fn add_misc_host(ip: String, domain: String, tag: String) -> Result<(), SkateError> {
    ensure_skatelet_dns_conf_dir();
    let log_tag = "add_misc_host";

    info!("{} dns add for {} {} # {}", log_tag, domain, ip, tag);

    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");

    lock(Box::new(move || {

        // scope to make sure files closed after
        {
            debug!("{} updating hosts file", log_tag);
            // create or open
            let mut addhosts_file = OpenOptions::new()
                .create(true)
                
                .append(true)
                .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

            // write with comment for now
            writeln!(addhosts_file, "{} {} # {}", ip, domain, tag).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
        }

        Ok(())
    }))
}

pub fn add(container_id: String, supplied_ip: Option<String>) -> Result<(), SkateError> {
    ensure_skatelet_dns_conf_dir();
    let log_tag = format!("{}::add", container_id);

    info!("{} dns add for {} {:?}", log_tag, container_id, supplied_ip);

    // TODO - store pod info in store, if no info, break retry loop
    let (extracted_ip, json) = retry(10, || {
        debug!("{} inspecting container {}",log_tag, container_id);
        let output = exec_cmd("timeout", &["0.2", "podman", "inspect", container_id.as_str()]).map_err(|e| (true, e))?;
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
        let output = exec_cmd("timeout", &["0.2", "podman", "pod", "inspect", pod.unwrap()]).map_err(|e| (true, e))?;
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
    let parent_resource = {
        if labels.contains_key("skate.io/daemonset") {
            Some("daemonset")
        } else if labels.contains_key("skate.io/deployment") {
            Some("deployment")
        } else {
            None
        }
    };
    
    if parent_resource.is_none() {
        info!("not a daemonset or deployment, skipping");
        return Ok(());
    }

    let parent_identifier_label = format!("skate.io/{}", parent_resource.unwrap());

    let app = labels.get(&parent_identifier_label).unwrap().as_str().unwrap();

    let domain = format!("{}.{}.pod.cluster.skate", app, ns);
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
                
                .append(true)
                .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

            // write with comment for now
            writeln!(addhosts_file, "#{} {} # {}", ip, domain, container_id_cpy).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
        }

        Ok(())
    }));

    if result.is_ok() {
        spawn_orphan_process("skatelet", ["dns", "enable", &container_id]);
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
    }).collect::<Vec<String>>().first().cloned()
}

pub fn wait_and_enable_healthy(container_id: String) -> Result<(), SkateError> {
    let log_tag = format!("{}::enable", container_id);
    debug!("{} inspecting container {}",log_tag, container_id);
    let output = exec_cmd("timeout", &["0.2", "podman", "inspect", container_id.as_str()])?;
    let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
    let pod = json[0]["Pod"].as_str();
    if pod.is_none() {
        warn!("{} no pod found", log_tag);
        return Err("no pod found".to_string().into());
    }

    debug!("{} inspecting pod", log_tag);
    let output = exec_cmd("timeout", &["0.2", "podman", "pod", "inspect", pod.unwrap()])?;
    let pod_json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))?;

    let containers: Vec<_> = pod_json["Containers"].as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
        c["Id"].as_str().unwrap()
    ).collect();

    let args = [vec!("0.2", "podman", "inspect"), containers].concat();

    let mut healthy = false;
    for _ in 0..60 {
        debug!("{} inspecting all pod containers",log_tag);
        let output = exec_cmd("timeout", &args)?;
        let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;

        // Check json for [*].State.Health.Status == "healthy"
        let containers: Vec<_> = json.as_array().ok_or_else(|| anyhow!("no containers found"))?.iter().map(|c|
            c["State"]["Health"]["Status"].as_str().unwrap()
        ).collect();

        if containers.iter().any(|c| *c == "unhealthy") {
            debug!("{} at least one container unhealthy",log_tag);
            // do nothing
            return Ok(());
        };

        if containers.into_iter().all(|c| c == "healthy" || c.is_empty()) {
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

            for line in reader.lines() {
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
    #[arg(long, long_help = "The container to remove dns entry for")]
    pub container_id: Option<String>,
    #[arg(long, long_help = "The pod to remove dns entry for")]
    pub pod_id: Option<String>,
}

// remove prints the ip of any dns entry that the container or pod had
pub fn remove(args: RemoveArgs) -> Result<(), SkateError> {
    let tag = {
        if args.container_id.is_some() {
            args.container_id.unwrap()
        } else if args.pod_id.is_some() {
            // get infra container
            let output = exec_cmd("podman", &["pod", "inspect", &args.pod_id.unwrap()])?;
            let json: serde_json::Value = serde_json::from_str(&output).map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
            let infra_container_id = json["InfraContainerID"].as_str().ok_or_else(|| anyhow!("no infra container found"))?;
            infra_container_id.to_string()
        } else {
            return Err(anyhow!("no container or pod id supplied").into());
        }
    };


    let log_tag = format!("{}::remove", tag);
    info!("{} removing dns entry for {}", log_tag, tag);
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

            for line in reader.lines() {
                let line = line?;
                if !line.ends_with(&tag) {
                    writeln!(writer, "{}", line)?;
                } else {
                    // ip is first column
                    let ip = line.split_whitespace().next().unwrap();
                    println!("{}", ip);
                }
            }
        }
        debug!("{} replacing hosts file", log_tag);
        fs::rename(&newaddnhosts_path, &addnhosts_path)?;
        Ok(())
    }))
}

pub fn reload() -> Result<(), SkateError> {
    let id = exec_cmd("podman", &["ps", "--filter", "label=skate.io/namespace=skate", "--filter", "label=skate.io/daemonset=coredns", "-q"])?;

    if id.is_empty() {
        return Err(anyhow!("no coredns container found").into());
    }

    // doesn't seem to work
    let _ = exec_cmd("podman", &["kill", "--signal", "HUP", &id])?;
    Ok(())
}