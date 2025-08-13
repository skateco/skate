use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::util::{SkateLabels, lock_file, spawn_orphan_process};
use anyhow::anyhow;
use log::{debug, error, info, warn};
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub struct DnsService<'a> {
    conf_path: String,
    execer: &'a Box<dyn ShellExec>,
}

impl<'a> DnsService<'a> {
    pub fn new(conf_path: &str, execer: &'a Box<dyn ShellExec>) -> Self {
        DnsService {
            conf_path: conf_path.to_string(),
            execer,
        }
    }

    fn lock<T>(&self, cb: Box<dyn FnOnce() -> Result<T, Box<dyn Error>>>) -> Result<T, SkateError> {
        let result = lock_file(&format!("{}/lock", self.conf_path), cb)?;
        Ok(result)
    }

    fn ensure_skatelet_dns_conf_dir(&self) {
        let conf_path = Path::new(&self.conf_path);

        fs::create_dir_all(conf_path).unwrap();
    }

    fn retry<T>(
        retries: u32,
        f: impl Fn() -> Result<T, (bool, Box<dyn Error>)>,
    ) -> Result<T, Box<dyn Error>> {
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
            Err((_, err)) => Err(err),
        }
    }

    pub fn add_misc_host(&self, ip: String, domain: String, tag: String) -> Result<(), SkateError> {
        self.ensure_skatelet_dns_conf_dir();
        let log_tag = "add_misc_host";

        info!("{} dns add for {} {} # {}", log_tag, domain, ip, tag);

        let addnhosts_path = Path::new(&self.conf_path).join("addnhosts");

        self.lock(Box::new(move || {
            // scope to make sure files closed after
            {
                debug!("{} updating hosts file", log_tag);
                // create or open
                let mut addhosts_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(addnhosts_path)
                    .map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

                // write with comment for now
                writeln!(addhosts_file, "{} {} # {}", ip, domain, tag)
                    .map_err(|e| anyhow!("failed to write host to file: {}", e))?;
            }

            Ok(())
        }))
    }

    pub fn add(&self, container_id: String, supplied_ip: Option<String>) -> Result<(), SkateError> {
        self.ensure_skatelet_dns_conf_dir();
        let log_tag = format!("{}::add", container_id);

        info!("{} dns add for {} {:?}", log_tag, container_id, supplied_ip);

        // TODO - store pod info in store, if no info, break retry loop
        let result = Self::retry(10, || {
            debug!("{} inspecting container {}", log_tag, container_id);
            let output = self
                .execer
                .exec(
                    "timeout",
                    &["0.2", "podman", "inspect", container_id.as_str()],
                    None,
                )
                .map_err(|e| (true, e))?;
            let container_json: serde_json::Value = serde_json::from_str(&output)
                .map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))
                .map_err(|e| (false, e.into()))?;
            // it's only the infra container that has the skate network
            // and ip
            let is_infra = container_json[0]["IsInfra"].as_bool().unwrap();
            if !is_infra {
                warn!("{} not infra container", log_tag);
                return Err((false, "not infra container".into()));
            }

            let ip = Self::extract_skate_ip(container_json[0].clone());

            let pod = container_json[0]["Pod"].as_str();
            if pod.is_none() {
                warn!("{} no pod found", log_tag);
                return Err((false, "no pod found".into()));
            }

            debug!("{} inspecting pod", log_tag);
            let output = self
                .execer
                .exec(
                    "timeout",
                    &["0.2", "podman", "pod", "inspect", pod.unwrap()],
                    None,
                )
                .map_err(|e| (true, e))?;
            let pod_json: serde_json::Value = serde_json::from_str(&output)
                .map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))
                .map_err(|e| (false, e.into()))?;
            Ok((ip, pod_json))
        });

        let (extracted_ip, json) = match result {
            Ok((extracted_ip, json)) => (extracted_ip, json),
            Err(err) => {
                if err.to_string().ends_with("not infra container") {
                    return Ok(());
                }
                return Err(err.into());
            }
        };

        let ip = match supplied_ip {
            Some(ip) => Some(ip),
            None => extracted_ip,
        };

        if ip.is_none() {
            warn!("{} no ip supplied or found for network 'skate'", log_tag);
            return Ok(());
        }
        let ip = ip.unwrap();

        let json = if json.is_array() {
            json[0].clone()
        } else {
            json
        };

        let labels = json["Labels"].as_object().unwrap();
        let ns = labels[&SkateLabels::Namespace.to_string()]
            .as_str()
            .ok_or_else(|| anyhow!("missing {} label", SkateLabels::Namespace))?;

        // only add for daemonsets or deployments
        let parent_resource = {
            if labels.contains_key(&SkateLabels::Daemonset.to_string()) {
                Some("daemonset")
            } else if labels.contains_key(&SkateLabels::Deployment.to_string()) {
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

        let app = labels
            .get(&parent_identifier_label)
            .unwrap()
            .as_str()
            .unwrap();

        let domain = format!("{}.{}.pod.cluster.skate", app, ns);
        let addnhosts_path = Path::new(&self.conf_path).join("addnhosts");

        let container_id_cpy = container_id.clone();
        // Do stuff
        let result = self.lock(Box::new(move || {
            // scope to make sure files closed after
            {
                debug!("{} updating hosts file", log_tag);
                // create or open
                let mut addhosts_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(addnhosts_path)
                    .map_err(|e| anyhow!("failed to open addnhosts file: {}", e))?;

                // write with comment for now
                writeln!(addhosts_file, "#{} {} # {}", ip, domain, container_id_cpy)
                    .map_err(|e| anyhow!("failed to write host to file: {}", e))?;
            }

            Ok(())
        }));

        if result.is_ok() {
            spawn_orphan_process("skatelet", ["dns", "enable", &container_id]);
        }
        result
    }

    fn extract_skate_ip(json: Value) -> Option<String> {
        json["NetworkSettings"]["Networks"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| {
                if k.eq("skate") {
                    match v["IPAddress"].as_str() {
                        Some(ip) => match ip {
                            "" => None,
                            _ => Some(ip.to_string()),
                        },
                        None => None,
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<String>>()
            .first()
            .cloned()
    }

    pub fn wait_and_enable_healthy(&self, container_id: String) -> Result<(), SkateError> {
        let log_tag = format!("{}::enable", container_id);
        debug!("{} inspecting container {}", log_tag, container_id);
        let output = self.execer.exec(
            "timeout",
            &["0.2", "podman", "inspect", container_id.as_str()],
            None,
        )?;
        let json: serde_json::Value = serde_json::from_str(&output)
            .map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
        let pod = json[0]["Pod"].as_str();
        if pod.is_none() {
            warn!("{} no pod found", log_tag);
            return Err("no pod found".to_string().into());
        }

        debug!("{} inspecting pod", log_tag);
        let output = self.execer.exec(
            "timeout",
            &["0.2", "podman", "pod", "inspect", pod.unwrap()],
            None,
        )?;
        let pod_json: serde_json::Value = serde_json::from_str(&output)
            .map_err(|e| anyhow!("failed to parse podman pod inspect output: {}", e))?;

        let pod_json = if pod_json.is_array() {
            pod_json[0].clone()
        } else {
            pod_json
        };

        let containers: Vec<_> = pod_json["Containers"]
            .as_array()
            .ok_or_else(|| anyhow!("no pod containers found"))?
            .iter()
            .map(|c| c["Id"].as_str().unwrap())
            .collect();

        if containers.is_empty() {
            warn!("{} no pod containers found", log_tag);
            return Ok(());
        }

        let containers_str = format!("{:?}", containers);
        let args = [vec!["0.2", "podman", "inspect"], containers].concat();

        let mut healthy = false;
        for _ in 0..60 {
            debug!("{} inspecting all pod containers", log_tag);
            let output = self.execer.exec("timeout", &args, None)?;
            let json: serde_json::Value = serde_json::from_str(&output)
                .map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;

            // Check json for [*].State.Health.Status == "healthy"
            let mut containers = json
                .as_array()
                .ok_or_else(|| anyhow!("no containers found while inspecting {}", containers_str))?
                .iter()
                .map(|c| {
                    c["State"]["Health"]["Status"].as_str().unwrap_or(
                        c["State"]["Status"].as_str().unwrap_or_else(|| {
                            error!("{} failed to find health status", log_tag);
                            "unknown"
                        }),
                    )
                });

            if containers.any(|c| c == "unhealthy") {
                debug!("{} at least one container unhealthy", log_tag);
                // do nothing
                return Ok(());
            };

            if containers.all(|c| c == "healthy" || c.is_empty()) {
                debug!("{} all containers healthy or no healthcheck", log_tag);
                healthy = true;
                break;
            };
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        if !healthy {
            warn!(
                "{} timed out waiting for all containers to be healthy",
                log_tag
            );
            return Ok(());
        }

        let addnhosts_path = Path::new(&self.conf_path).join("addnhosts");
        let newaddnhosts_path = Path::new(&self.conf_path).join("addnhosts-new");

        self.lock(Box::new(move || {
            // scope to make sure files closed after
            {
                // create or open

                let addhosts_file = OpenOptions::new().read(true).open(addnhosts_path.clone());

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
                        debug!("{} enabling dns entry for {}", log_tag, container_id);
                        writeln!(writer, "{}", line.trim().trim_start_matches('#'))?;
                    } else {
                        writeln!(writer, "{}", line)?;
                    }
                }
            }
            debug!("{} replacing hosts file", log_tag);
            fs::rename(&newaddnhosts_path, &addnhosts_path)?;
            Ok(())
        }))
    }

    // remove prints the ip of any dns entry that the container or pod had
    pub fn remove(
        &self,
        container_id: Option<String>,
        pod_id: Option<String>,
    ) -> Result<(), SkateError> {
        let tag = {
            if let Some(id) = container_id {
                id
            } else if pod_id.is_some() {
                // get infra container
                let output =
                    self.execer
                        .exec("podman", &["pod", "inspect", &pod_id.unwrap()], None)?;
                let json: serde_json::Value = serde_json::from_str(&output)
                    .map_err(|e| anyhow!("failed to parse podman inspect output: {}", e))?;
                let infra_container_id = json["InfraContainerID"]
                    .as_str()
                    .ok_or_else(|| anyhow!("no infra container found"))?;
                infra_container_id.to_string()
            } else {
                return Err(anyhow!("no container or pod id supplied").into());
            }
        };

        let log_tag = format!("{}::remove", tag);
        info!("{} removing dns entry for {}", log_tag, tag);
        self.ensure_skatelet_dns_conf_dir();
        let addnhosts_path = Path::new(&self.conf_path).join("addnhosts");
        let newaddnhosts_path = Path::new(&self.conf_path).join("addnhosts-new");

        // Do stuff
        self.lock(Box::new(move || {
            // scope to make sure files closed after
            {
                // create or open

                let addhosts_file = OpenOptions::new().read(true).open(addnhosts_path.clone());

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

    pub fn reload(&self) -> Result<(), SkateError> {
        let id = self.execer.exec(
            "podman",
            &[
                "ps",
                "--filter",
                "label=skate.io/namespace=skate",
                "--filter",
                "label=skate.io/daemonset=coredns",
                "-q",
            ],
            None,
        )?;

        if id.is_empty() {
            return Err(anyhow!("no coredns container found").into());
        }

        // doesn't seem to work
        let _ = self
            .execer
            .exec("podman", &["kill", "--signal", "HUP", &id], None)?;
        Ok(())
    }
}
