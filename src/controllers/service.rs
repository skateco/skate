use std::error::Error;
use std::fs;
use std::net::Ipv4Addr;
use std::str::FromStr;
use anyhow::anyhow;
use k8s_openapi::api::core::v1::Service;
use log::info;
use serde_json::{json, Value};
use crate::{filestore, template};
use crate::filestore::FileStore;
use crate::skate::exec_cmd;
use crate::skatelet::dns;
use crate::skatelet::dns::RemoveArgs;
use crate::util::{lock_file, metadata_name};

pub struct ServiceController {
    store: FileStore
}

impl ServiceController {
    pub fn new(store: FileStore) -> Self {
        ServiceController {
            store: store,
        }
    }

    pub fn apply(&self, service: Service) -> Result<(), Box<dyn Error>> {
        let manifest_string = serde_yaml::to_string(&service).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        let name = &metadata_name(&service).to_string();

        // manifest goes into store
        let yaml_path = self.store.write_file("service", name, "manifest.yaml", manifest_string.as_bytes())?;

        let hash = service.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("service", &name, "hash", &hash.as_bytes())?;
        }

        // install systemd service and timer
        let mut handlebars = template::new();
        ////////////////////////////////////////////////////
        // template cron-pod.service to /var/lib/state/store/cronjob/<name>/systemd.service
        ////////////////////////////////////////////////////

        handlebars.register_template_string("unit", include_str!("../resources/skate-ipvsmon.service")).map_err(|e| anyhow!(e).context("failed to load service template file"))?;


        // cidr is 10.30.0.0/16
        // we just keep incrementing
        // reserve 10.30.0.1 for the empty lvs we have in the root keepalived conf to make it start
        let service_subnet_start = "10.30.0.1";

        let ip = lock_file("/var/lib/skate/keepalived/service-ips.lock", Box::new(move || {
            info!("reading ip file");


            let last_ip = fs::read_to_string("/var/lib/skate/keepalived/service-ips").unwrap_or_default();
            info!("converting {} to Ipv4Addr", last_ip);
            let last_ip = Ipv4Addr::from_str(&last_ip).unwrap_or_else(|_| Ipv4Addr::from_str(service_subnet_start).unwrap());

            info!("last ip: {}", last_ip);

            let mut octets = last_ip.octets();

            if octets[3] == 255 {
                if octets[2] == 255 {
                    return Err(anyhow!("no more ips available on subnet {}/16", service_subnet_start).into());
                }
                octets[2] += 1;
                octets[3] = 0;
            } else {
                octets[3] += 1;
            }

            let ip = Ipv4Addr::from(octets);

            fs::write("/var/lib/skate/keepalived/service-ips", ip.to_string())?;

            Ok(ip.to_string())
        }))?;

        let json = json!({
            "svc_name":name,
            "ip": ip,
            "yaml_path": yaml_path,
        });

        let file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-ipvsmon-{}.service", &name))?;
        handlebars.render_to_write("unit", &json, file)?;

        handlebars.register_template_string("timer", include_str!("../resources/skate-ipvsmon.timer")).map_err(|e| anyhow!(e).context("failed to load timer template file"))?;
        let json: Value = json!({
            "svc_name":name,
        });
        let file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-ipvsmon-{}.timer", &name))?;
        handlebars.render_to_write("timer", &json, file)?;
        let unit_name = format!("skate-ipvsmon-{}", &name);

        exec_cmd("systemctl", &["daemon-reload"])?;
        exec_cmd("systemctl", &["enable", "--now", &unit_name])?;
        exec_cmd("systemctl", &["reset-failed", &unit_name])?;

        let domain = format!("{}.svc.cluster.skate", name);
        dns::add_misc_host(ip, domain.clone(), domain)?;

        Ok(())
    }

    pub fn delete(&self, service: Service) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&service);
        dns::remove(RemoveArgs { container_id: Some(format!("{}.svc.cluster.skate", ns_name)), pod_id: None })?;

        let _ = exec_cmd("systemctl", &["stop", &format!("skate-ipvsmon-{}", &ns_name.to_string())]);

        let _ = exec_cmd("systemctl", &["disable", &format!("skate-ipvsmon-{}", &ns_name.to_string())]);
        let _ = exec_cmd("rm", &[&format!("/etc/systemd/system/skate-ipvsmon-{}.service", &ns_name.to_string())]);
        let _ = exec_cmd("rm", &[&format!("/etc/systemd/system/skate-ipvsmon-{}.timer", &ns_name.to_string())]);
        let _ = exec_cmd("rm", &[&format!("/var/lib/skate/keepalived/{}.conf", &ns_name.to_string())]);
        let _ = exec_cmd("systemctl", &["daemon-reload"])?;
        let _ = exec_cmd("systemctl", &["reset-failed"])?;

        let _ = self.store.remove_object("service", &ns_name.to_string())?;

        Ok(())
    }
}