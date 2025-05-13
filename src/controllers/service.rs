use crate::exec::ShellExec;
use crate::skatelet::database::resource::{
    delete_resource, insert_resource, Resource, ResourceType,
};
use crate::skatelet::services::dns::DnsService;
use crate::template;
use crate::util::{lock_file, metadata_name};
use anyhow::anyhow;
use k8s_openapi::api::core::v1::Service;
use log::{error, info};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::error::Error;
use std::fs;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub struct ServiceController {
    db: SqlitePool,
    execer: Box<dyn ShellExec>,
    skate_var_path: String,    // /var/lib/skate
    systemd_unit_path: String, // always /etc/systemd/system
}

impl ServiceController {
    pub fn new(
        db: SqlitePool,
        execer: Box<dyn ShellExec>,
        var_path: &str,
        systemd_etc_path: &str,
    ) -> Self {
        ServiceController {
            db,
            execer,
            skate_var_path: var_path.to_string(),
            systemd_unit_path: systemd_etc_path.to_string(),
        }
    }

    pub async fn apply(&self, service: &Service) -> Result<(), Box<dyn Error>> {
        let manifest = serde_json::to_value(service)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to json"))?;

        let name = metadata_name(service);
        if name.name == "" || name.namespace == "" {
            return Err(anyhow!("invalid name or namespace: {}", name).into());
        }

        // manifest goes into store

        let hash = service
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        let object = Resource {
            name: name.name.clone(),
            namespace: name.namespace.clone(),
            resource_type: ResourceType::Service,
            manifest,
            hash,
            ..Default::default()
        };

        insert_resource(&self.db, &object).await?;

        // install systemd service and timer
        let mut handlebars = template::new();
        ////////////////////////////////////////////////////
        // template cron-pod.service to /var/lib/state/store/cronjob/<name>/systemd.service
        ////////////////////////////////////////////////////

        handlebars
            .register_template_string("unit", include_str!("../resources/skate-ipvsmon.service"))
            .map_err(|e| anyhow!(e).context("failed to load service template file"))?;

        // cidr is 10.30.0.0/16
        // we just keep incrementing
        // reserve 10.30.0.1 for the empty lvs we have in the root keepalived conf to make it start
        let service_subnet_start = "10.30.0.1";

        let lock_path = format!("{}/keepalived/service-ips.lock", self.skate_var_path);
        let ips_path = format!("{}/keepalived/service-ips", self.skate_var_path);

        let ip = lock_file(
            &lock_path,
            Box::new(move || {
                info!("reading ip file");

                let last_ip = fs::read_to_string(&ips_path).unwrap_or_default();
                info!("converting {} to Ipv4Addr", last_ip);
                let last_ip = Ipv4Addr::from_str(&last_ip)
                    .unwrap_or_else(|_| Ipv4Addr::from_str(service_subnet_start).unwrap());

                info!("last ip: {}", last_ip);

                let mut octets = last_ip.octets();

                if octets[3] == 255 {
                    if octets[2] == 255 {
                        return Err(anyhow!(
                            "no more ips available on subnet {}/16",
                            service_subnet_start
                        )
                        .into());
                    }
                    octets[2] += 1;
                    octets[3] = 0;
                } else {
                    octets[3] += 1;
                }

                let ip = Ipv4Addr::from(octets);

                fs::write(&ips_path, ip.to_string())?;

                Ok(ip.to_string())
            }),
        )?;

        let json = json!({
            "svc_name":name.to_string(),
            "ip": ip,
        });

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!(
                "{}/skate-ipvsmon-{}.service",
                self.systemd_unit_path, &name
            ))?;

        handlebars.render_to_write("unit", &json, file)?;

        handlebars
            .register_template_string("timer", include_str!("../resources/skate-ipvsmon.timer"))
            .map_err(|e| anyhow!(e).context("failed to load timer template file"))?;
        let json: Value = json!({
            "svc_name":name.to_string(),
        });
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!(
                "{}/skate-ipvsmon-{}.timer",
                self.systemd_unit_path, &name
            ))?;
        handlebars.render_to_write("timer", &json, file)?;
        let unit_name = format!("skate-ipvsmon-{}", &name);

        self.execer.exec("systemctl", &["daemon-reload"], None)?;
        self.execer
            .exec("systemctl", &["enable", "--now", &unit_name], None)?;
        self.execer
            .exec("systemctl", &["reset-failed", &unit_name], None)?;

        let domain = format!("{}.svc.cluster.skate", name);
        let dns = DnsService::new(&format!("{}/dns", self.skate_var_path), &self.execer);
        dns.add_misc_host(ip, domain.clone(), domain)?;

        Ok(())
    }

    pub async fn delete(&self, service: &Service) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(service);
        let dns = DnsService::new(&format!("{}/dns", self.skate_var_path), &self.execer);
        dns.remove(Some(format!("{}.svc.cluster.skate", ns_name)), None)?;

        let res = self.execer.exec(
            "systemctl",
            &["stop", &format!("skate-ipvsmon-{}", &ns_name.to_string())],
            None,
        );
        if res.is_err() {
            error!("failed to stop {} ipvsmon: {}", ns_name, res.unwrap_err());
        }

        let res = self.execer.exec(
            "systemctl",
            &[
                "disable",
                &format!("skate-ipvsmon-{}", &ns_name.to_string()),
            ],
            None,
        );
        if res.is_err() {
            error!(
                "failed to disable {} ipvsmon: {}",
                ns_name,
                res.unwrap_err()
            );
        }

        let res = self.execer.exec(
            "rm",
            &[&format!(
                "{}/skate-ipvsmon-{}.service",
                self.systemd_unit_path,
                &ns_name.to_string()
            )],
            None,
        );
        if res.is_err() {
            error!(
                "failed to remove {} ipvsmon service: {}",
                ns_name,
                res.unwrap_err()
            );
        }
        let res = self.execer.exec(
            "rm",
            &[&format!(
                "{}/skate-ipvsmon-{}.timer",
                self.systemd_unit_path,
                &ns_name.to_string()
            )],
            None,
        );
        if res.is_err() {
            error!(
                "failed to remove {} ipvsmon timer: {}",
                ns_name,
                res.unwrap_err()
            );
        }

        let res = self.execer.exec(
            "rm",
            &[&format!(
                "{}/keepalived/{}.conf",
                self.skate_var_path,
                &ns_name.to_string()
            )],
            None,
        );
        if res.is_err() {
            error!(
                "failed to remove {} keepalived conf: {}",
                ns_name,
                res.unwrap_err()
            );
        }

        self.execer.exec("systemctl", &["daemon-reload"], None)?;
        self.execer.exec("systemctl", &["reset-failed"], None)?;

        delete_resource(
            &self.db,
            &ResourceType::Service,
            &ns_name.name,
            &ns_name.namespace,
        )
        .await?;

        Ok(())
    }
}
