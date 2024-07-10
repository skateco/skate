use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use anyhow::anyhow;
use clap::{Args, Subcommand};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use crate::skatelet::skatelet::VAR_PATH;

#[derive(Debug, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    command: HookCommand,
}


#[derive(Debug, Subcommand)]
pub enum HookCommand {
    #[command(name = "prestart", about = "prestart hook")]
    Prestart,
    #[command(name = "poststop", about = "poststop hook")]
    Poststop,
}

pub fn oci(apply_args: HookArgs) -> Result<(), Box<dyn Error>> {
    match apply_args.command {
        HookCommand::Prestart => pre_start(),
        HookCommand::Poststop => post_stop()
    }
}

#[derive(Serialize, Deserialize)]
struct config {
    annotations: HashMap<String, String>,
}

fn lock<T>(lock_path: &PathBuf, cb: &dyn Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    let lock_file = File::create(lock_path.clone()).map_err(|e| anyhow!("failed to create/open lock file {}: {}",lock_path.to_string_lossy(), e))?;
    lock_file.lock_exclusive()?;
    let result = cb();
    lock_file.unlock()?;
    result
}

fn pre_start() -> Result<(), Box<dyn Error>> {
    let config_file = File::open("./config.json").map_err(|e| anyhow!("failed to open config.json: {}", e))?;
    let conf: config = serde_json::from_reader(config_file).map_err(|e| anyhow!("failed to read config.json: {}", e))?;
    let ns = conf.annotations.get("skate.io/namespace");

    if ns.is_none() {
        return Ok(());
    }

    let _ns = ns.unwrap();


    let cwd = env::current_dir().map_err(|e| anyhow!("failed to get cwd: {}", e))?;
    let _container_id = cwd.parent().unwrap().file_name().unwrap().to_str().unwrap();

    let dns_path = format!("{}/dns", VAR_PATH);

    lock(&Path::new(&dns_path).join("lock"), &|| {
        let addnhosts_path = Path::new(&dns_path).join("addnhosts");
        // create or open
        let _addhosts_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(addnhosts_path).map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

        let _names: Vec<String> = vec![];


        // if result.ips.len() == 0 {
        //     return Err("no ips in prev_result".into());
        // }
        //
        // let ip_str = result.ips[0].address.ip().to_string();
        //
        // for name in names {
        //     writeln!(addhosts_file, "{} {}", ip_str, name).map_err(|e| anyhow!("failed to write host to file: {}", e))?;
        // }
        Ok(())
    })?;

    // // write to /var/lib/skatelet/pods/<id>/ns/<ns>
    // let dir = format!("{}/containers/{}", VAR_PATH, container_id);
    // create_dir_all(dir.clone()).map_err(|e| anyhow!("failed to create container dir: {}", e))?;
    //
    // let mut file = File::create(format!("{}/ns", dir)).map_err(|e| anyhow!("failed to create container ns file: {}", e))?;
    // file.write(ns.as_bytes()).map_err(|e| anyhow!("failed to write container ns file: {}", e))?;
    Ok(())
}

fn list_sub_dirs(path: &str) -> Vec<String> {
    let dir = std::fs::read_dir(path).map_err(|e| anyhow!("failed to read container dir: {}", e));
    if dir.is_err() {
        return vec![];
    }
    let dir = dir.unwrap();

    return dir.filter_map(|d| {
        if let Ok(d) = d {
            if let Ok(t) = d.file_type() {
                if t.is_dir() {
                    return Some(d.file_name().to_string_lossy().to_string());
                }
            }
        }
        return None;
    }).collect();
}

fn post_stop() -> Result<(), Box<dyn Error>> {
    // let cwd = env::current_dir().map_err(|e| anyhow!("failed to get cwd: {}", e))?;
    //
    // let skate_containers = list_sub_dirs(format!("{}/containers", VAR_PATH).as_str());
    // // TODO - use podman cli
    // let podman_containers = list_sub_dirs("/var/lib/containers/storage/overlay-containers");
    //
    // println!("skate_containers: {:?}", skate_containers);
    // println!("podman_containers: {:?}", podman_containers);
    //
    // for skate_container in skate_containers {
    //     if !podman_containers.contains(&skate_container) {
    //         let dir = format!("{}/containers/{}", VAR_PATH, skate_container);
    //         remove_dir_all(dir.clone()).map_err(|e| anyhow!("failed to remove container dir {}: {}",dir, e))?;
    //     }
    // }
    //
    Ok(())
}
