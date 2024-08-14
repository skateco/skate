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


#[derive(Debug,Subcommand)]
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
        Command::Add(add_args) => add(add_args.container_id, add_args.ip, NamespacedName { name: add_args.name, namespace: add_args.namespace }),
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
    name: String,
    namespace: String,
}

pub fn add(container_id: String, ip: String, namespaced_name: NamespacedName) -> Result<(), Box<dyn Error>> {
    ensure_skatelet_dns_conf_dir();
    let app = namespaced_name.name;
    let ns = namespaced_name.namespace;

    let domain = format!("{}.{}.cluster.skate", app.clone(), ns.clone());
    let addnhosts_path = Path::new(&conf_path_str()).join("addnhosts");
    // Do stuff
    lock(Box::new(move || {

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