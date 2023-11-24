use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use cni_plugin::Cni;
use cni_plugin::reply::{ErrorReply, reply, SuccessReply};
use fs2::FileExt;
use log::{debug, info, warn, error};
use std::io::prelude::*;
use serde_json::Value;
use serde_json::Value::String;

const DEFAULT_CONF_PATH: &str = "/run/containers/cni/skatelet/";

fn lock<T>(ifname: &str, cb: &dyn Fn() -> Result<T, Box<dyn Error>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(DEFAULT_CONF_PATH).join(ifname.clone()).join("lock");
    let lock_file = File::open(lock_path.clone())?;
    debug!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    debug!("locked {}", lock_path.display());

    let result = cb();

    lock_file.unlock()?;

    result
}

pub fn cni() {
    match Cni::load() {
        Cni::Add { container_id, ifname, netns, path, config } => {
            // touch lock file at DEFAULT_CONF_PATH/<interface>/lock

            match lock(&ifname, &|| {
                // read file at DEFAULT_CONF_PATH/<interface>/addnhosts
                let addnhosts_path = Path::new(DEFAULT_CONF_PATH).join(ifname.clone()).join("addnhosts");
                // create or open
                let mut addhosts_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(true)
                    .open(addnhosts_path)
                    .unwrap();

                let mut names = config.runtime.as_ref().and_then(|r| Some(r.aliases.clone())).unwrap_or_default();
                let pod_name = config.args.get("podname").unwrap_or(&String("".to_string())).to_string();
                names.push(pod_name);

                let prev_result: SuccessReply = serde_json::from_value((*config.prev_result.as_ref().unwrap_or(&Value::Null)).clone())?;

                if prev_result.ips.len() == 0 {
                    return Err("no ips in prev_result".into());
                }

                debug!("{:?}", config.args);
                // TODO read namespace

                for name in names {
                    writeln!(addhosts_file, "{} {}", prev_result.ips[0].address.to_string(), name).unwrap();
                }
                Ok(())
            }) {
                Ok(_) => {
                    reply(SuccessReply {
                        cni_version: config.cni_version,
                        interfaces: Default::default(),
                        ips: Default::default(),
                        routes: Default::default(),
                        dns: Default::default(),
                        specific: Default::default(),
                    });
                }
                Err(e) => {
                    reply(ErrorReply {
                        cni_version: config.cni_version,
                        code: 1, // TODO
                        msg: &e.to_string(),
                        details: "".to_string(),
                    })
                }
            }
        }
        Cni::Del { container_id, ifname, netns, path, config } => {
            match lock(&ifname, &|| {
                // read file at DEFAULT_CONF_PATH/<interface>/addnhosts
                let addnhosts_path = Path::new(DEFAULT_CONF_PATH).join(ifname.clone()).join("addnhosts");
                let newaddnhosts_path = Path::new(DEFAULT_CONF_PATH).join(ifname.clone()).join("addnhosts-new");
                // scope to make sure files closed after
                {
                    // create or open

                    let addhosts_file = OpenOptions::new()
                        .read(true)
                        .open(addnhosts_path.clone())
                        .unwrap();

                    let mut newaddhosts_file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(newaddnhosts_path.clone())
                        .unwrap();

                    let reader = BufReader::new(&addhosts_file);
                    let mut writer = BufWriter::new(&newaddhosts_file);

                    let prev_result: SuccessReply = serde_json::from_value((*config.prev_result.as_ref().unwrap_or(&Value::Null)).clone())?;

                    if prev_result.ips.len() == 0 {
                        return Err("no ips in prev_result".into());
                    }

                    let ip = prev_result.ips[0].address.to_string();

                    for (index, line) in reader.lines().enumerate() {
                        let line = line.as_ref().unwrap();
                        if !line.starts_with(&ip) {
                            writeln!(writer, "{}", line)?;
                        }
                    }
                }
                fs::rename(&newaddnhosts_path, &addnhosts_path).unwrap();
                Ok(())
            }) {
                Ok(_) => {
                    reply(SuccessReply {
                        cni_version: config.cni_version,
                        interfaces: Default::default(),
                        ips: Default::default(),
                        routes: Default::default(),
                        dns: Default::default(),
                        specific: Default::default(),
                    });
                }
                Err(e) => {
                    reply(ErrorReply {
                        cni_version: config.cni_version,
                        code: 1, // TODO
                        msg: &e.to_string(),
                        details: "".to_string(),
                    })
                }
            }
        }
        Cni::Check { container_id, ifname, netns, path, config } => {
            // check addnhosts file exists
            reply(SuccessReply {
                cni_version: config.cni_version,
                interfaces: Default::default(),
                ips: Default::default(),
                routes: Default::default(),
                dns: Default::default(),
                specific: Default::default(),
            });
        }
        Cni::Version(_) => unreachable!()
    }
}