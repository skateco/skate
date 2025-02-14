use crate::deps::With;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::util::NamespacedName;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use handlebars::Handlebars;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Service;
use log::info;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::prelude::*;
use std::net::ToSocketAddrs;
use std::path::Path;

const TERMINATED_MAX_AGE: i64 = 300; // seconds
const CLEANUP_INTERVAL: i64 = 30; //seconds

#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    #[command(about = "synchronise a service's ips")]
    Sync(SyncArgs),
    #[command(about = "disable a service's target ips")]
    DisableIp(DisableIpArgs),
}

#[derive(Debug, Args)]
pub struct IpvsArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Debug, Args)]
pub struct DisableIpArgs {
    host: String,
    ips: Vec<String>,
}

#[derive(Clone, Debug, Args)]
pub struct SyncArgs {
    #[arg(long, long_help = "Name of the file to write keepalived config to.")]
    out: String,
    host: String,
    file: String,
}

pub trait IPVSDeps: With<dyn ShellExec> {}

pub struct IPVS<D: IPVSDeps> {
    pub deps: D,
}

impl<D: IPVSDeps> IPVS<D> {
    pub fn ipvs(&self, args: IpvsArgs) -> Result<(), SkateError> {
        match args.command {
            Commands::Sync(args) => self.sync(args),
            Commands::DisableIp(args) => self.disable_ips(args),
        }
    }
    fn disable_ips(&self, args: DisableIpArgs) -> Result<(), SkateError> {
        Self::terminated_add(&args.host, &args.ips)
    }

    fn sync(&self, args: SyncArgs) -> Result<(), SkateError> {
        // args.service_name is fqn like foo.bar
        let mut manifest: Service = serde_yaml::from_str(&fs::read_to_string(args.file)?)?;
        let spec = manifest.spec.clone().unwrap_or_default();
        let name = spec
            .selector
            .unwrap_or_default()
            .get("app.kubernetes.io/name")
            .unwrap_or(&"".to_string())
            .clone();
        if name.is_empty() {
            return Err(anyhow!("service selector app.kubernetes.io/name is required").into());
        }
        let ns = manifest.metadata.namespace.unwrap_or("default".to_string());
        let fqn = NamespacedName {
            name,
            namespace: ns.clone(),
        };

        Self::cleanup(&fqn.to_string());

        manifest.metadata.namespace = Some(ns);

        // 80 is just to have a port, could be anything
        let domain = format!("{}.pod.cluster.skate:80", fqn);
        // get all pod ips from dns <args.service_name>.cluster.skate
        info!("looking up ips for {}", &domain);
        let addrs: HashSet<_> = domain
            .to_socket_addrs()
            .unwrap_or_default()
            .map(|addr| addr.ip().to_string())
            .collect();

        let terminating = Self::terminated_list(&fqn.to_string())?;

        let something_changed = Self::hash_changed(&addrs, &terminating, &fqn.to_string())?
            || !Path::new(&args.out).exists();
        // hashes match and output file exists
        if !something_changed {
            info!("no changes detected: {:?}", &addrs);
            return Ok(());
        }

        info!(
            "changes detected, rewriting keepalived config for {} -> {:?}",
            &args.host, &addrs
        );
        // check the old ADD ips in the cache file (remove those with a DEL line)
        let last_result = Self::lastresult_list(&fqn.to_string())?;
        Self::lastresult_save(
            &fqn.to_string(),
            &addrs.iter().cloned().collect::<Vec<String>>(),
        )?;
        // remove deleted items that are in the latest result
        let deleted = terminating
            .iter()
            .filter(|&i| !addrs.contains(i))
            .cloned()
            .collect::<Vec<String>>();

        // find those that are now missing, add those to the cache file under DEL
        let missing_now: Vec<_> = last_result.difference(&addrs).cloned().collect();
        Self::terminated_add(&fqn.to_string(), &missing_now)?;

        let new: Vec<_> = addrs.difference(&last_result).cloned().collect();
        info!("added: {:?}", new);
        info!("deleted: {:?}", missing_now);

        // append newly missing to deleted list
        let deleted = [deleted, missing_now].concat();

        // rewrite keepalived include file
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true);

        handlebars
            .register_template_string(
                "keepalived",
                include_str!("../resources/keepalived-service.conf"),
            )
            .map_err(|e| anyhow!(e).context("failed to load keepalived file"))?;

        // write config
        {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(args.out)?;
            handlebars.render_to_write(
                "keepalived",
                &json!({
                    "host": args.host,
                    "manifest": manifest,
                    "target_ips": addrs,
                    "deleted_ips": deleted,
                }),
                file,
            )?;
        }

        // reload keepalived
        let _ =
            With::<dyn ShellExec>::get(&self.deps).exec("systemctl", &["reload", "keepalived"])?;
        Ok(())
    }

    fn hash_changed(
        addrs: &HashSet<String>,
        terminating: &HashSet<String>,
        service_name: &str,
    ) -> Result<bool, SkateError> {
        let mut addrs_hasher = DefaultHasher::new();
        let addrs: Vec<_> = addrs.iter().cloned().sorted().collect();
        addrs.hash(&mut addrs_hasher);

        let mut terminating_hasher = DefaultHasher::new();
        let deleted: Vec<_> = terminating.iter().cloned().sorted().collect();
        deleted.hash(&mut terminating_hasher);

        let new_hash = format!(
            "{:x}|{:x}",
            addrs_hasher.finish(),
            terminating_hasher.finish()
        );
        let hash_file_name = format!("/run/skatelet-ipvsmon-{}.hash", service_name);

        let old_hash = fs::read_to_string(&hash_file_name).unwrap_or_default();

        let changed = old_hash != new_hash;
        if changed {
            fs::write(&hash_file_name, new_hash)?;
        }
        Ok(changed)
    }

    fn cleanup(service_name: &str) -> bool {
        let cleanup_file = Self::cleanup_last_run_file_name(service_name);
        let now = chrono::Utc::now().timestamp();
        let last_run = fs::read_to_string(&cleanup_file)
            .unwrap_or_default()
            .parse::<i64>()
            .unwrap_or_default();
        if now - last_run > CLEANUP_INTERVAL {
            let changed = Self::cleanup_terminated_list(service_name).unwrap_or_else(|e| {
                info!("failed to clean up terminated list: {}", e);
                false
            });

            if let Err(e) = fs::write(cleanup_file, format!("{}", now)) {
                info!("failed to write cleanup file: {}", e);
            };
            return changed;
        }
        false
    }

    fn terminated_list_file_name(service_name: &str) -> String {
        format!("/run/skatelet-ipvsmon-{}.terminated", service_name)
    }

    fn last_result_file_name(service_name: &str) -> String {
        format!("/run/skatelet-ipvsmon-{}.lastresult", service_name)
    }
    fn cleanup_last_run_file_name(service_name: &str) -> String {
        format!("/run/skatelet-ipvsmon-{}.cleanup", service_name)
    }

    fn lastresult_save(service_name: &str, ips: &[String]) -> Result<(), SkateError> {
        fs::write(Self::last_result_file_name(service_name), ips.join("\n"))?;
        Ok(())
    }
    fn lastresult_list(service_name: &str) -> Result<HashSet<String>, SkateError> {
        let contents = match fs::read_to_string(Self::last_result_file_name(service_name)) {
            Ok(contents) => contents,
            Err(_) => return Ok(HashSet::new()),
        };

        Ok(contents.lines().map(|i| i.to_string()).collect())
    }

    fn terminated_add(service_name: &str, ips: &Vec<String>) -> Result<(), SkateError> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(Self::terminated_list_file_name(service_name))?;
        for ip in ips {
            file.write_all(format!("{} {}\n", ip, chrono::Utc::now().timestamp()).as_bytes())?;
        }
        Ok(())
    }

    fn terminated_list(service_name: &str) -> Result<HashSet<String>, SkateError> {
        let now = chrono::Utc::now().timestamp();

        let contents = match fs::read_to_string(Self::terminated_list_file_name(service_name)) {
            Ok(contents) => contents,
            Err(_) => return Ok(HashSet::new()),
        };

        let mut deleted = HashSet::new();

        for line in contents.lines().sorted().rev() {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() != 2 {
                continue;
            }
            let ip = parts[0];
            let ts = parts[1];

            if now - ts.parse::<i64>().map_err(|e| anyhow!(e))? > TERMINATED_MAX_AGE {
                continue;
            }

            if deleted.contains(ip) {
                continue;
            }
            deleted.insert(ip.to_string());
        }

        Ok(deleted)
    }
    // TODO - remove straight away if ipvs active + inactive conns = 0
    fn cleanup_terminated_list(service_name: &str) -> Result<bool, SkateError> {
        info!("cleaning up terminated list for {}", service_name);
        let contents = match fs::read_to_string(Self::terminated_list_file_name(service_name)) {
            Ok(contents) => contents,
            Err(_) => return Ok(false),
        };

        let mut new_contents = String::new();
        // want DEL lines first

        let now = chrono::Utc::now().timestamp();

        let mut keep_set = HashSet::new();

        let mut changed = false;

        for line in contents.lines().sorted().rev() {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() != 2 {
                continue;
            }
            let ip = parts[0];
            let ts = parts[1].parse::<i64>().map_err(|e| anyhow!(e))?;

            if keep_set.contains(ip) {
                changed = true;
                continue;
            }
            // terminated less than 5 minutes ago
            if now - ts < TERMINATED_MAX_AGE {
                info!(
                    "keeping {} since it was terminated {} seconds ago ( < {} seconds ago )",
                    ip,
                    TERMINATED_MAX_AGE,
                    now - ts
                );
                keep_set.insert(ip);
                new_contents.push_str(line);
                new_contents.push('\n');
                continue;
            }

            changed = true
        }

        if changed {
            fs::write(Self::terminated_list_file_name(service_name), new_contents)?;
        }

        Ok(changed)
    }
}
