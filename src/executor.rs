use std::error::Error;
use std::fs::File;
use std::io::{BufRead, Write};
use std::net::Ipv4Addr;
use std::{fs, process};
use std::process::Stdio;
use std::str::FromStr;
use anyhow::anyhow;
use handlebars::Handlebars;
use itertools::Itertools;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Pod, Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use log::info;
use serde_json::{json, Value};

use crate::cron::cron_to_systemd;
use crate::filestore::FileStore;
use crate::skate::{exec_cmd, SupportedResources};
use crate::skatelet::dns;
use crate::skatelet::dns::RemoveArgs;
use crate::spec::cert::ClusterIssuer;
use crate::template;
use crate::util::{hash_string, lock_file, metadata_name};

pub trait Executor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>>;
    fn manifest_delete(&self, object: SupportedResources, grace: Option<usize>) -> Result<(), Box<dyn Error>>;
}

pub struct DefaultExecutor {
    store: FileStore,
}


impl DefaultExecutor {
    pub fn new() -> Self {
        DefaultExecutor {
            store: FileStore::new(),
        }
    }

    fn write_manifest_to_file(manifest: &str) -> Result<String, Box<dyn Error>> {
        let file_path = format!("/tmp/skate-{}.yaml", hash_string(manifest));
        let mut file = File::create(file_path.clone()).expect("failed to open file for manifests");
        file.write_all(manifest.as_ref()).expect("failed to write manifest to file");
        Ok(file_path)
    }

    fn reload_ingress(&self) -> Result<(), Box<dyn Error>> {

        // trigger SIGHUP to ingress container
        // sudo bash -c "podman kill --signal HUP \$(podman ps --filter label=skate.io/namespace=skate --filter label=skate.io/daemonset=nginx-ingress -q)"
        let id = exec_cmd("podman", &["ps", "--filter", "label=skate.io/namespace=skate", "--filter", "label=skate.io/daemonset=nginx-ingress", "-q"])?;

        if id.is_empty() {
            return Err(anyhow!("no ingress container found").into());
        }

        let _ = exec_cmd("podman", &["kill", "--signal", "HUP", &format!("{}", id)])?;
        Ok(())
    }

    fn apply_cronjob(&self, cron_job: CronJob) -> Result<(), Box<dyn Error>> {
        let cron_job_string = serde_yaml::to_string(&cron_job).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(&cron_job);

        self.store.write_file("cronjob", &ns_name.to_string(), "manifest.yaml", cron_job_string.as_bytes())?;

        let hash = cron_job.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("cronjob", &ns_name.to_string(), "hash", &hash.as_bytes())?;
        }

        let spec = cron_job.spec.clone().unwrap_or_default();
        let timezone = spec.time_zone.unwrap_or_default();

        let systemd_timer_schedule = cron_to_systemd(&spec.schedule, &timezone)?;

        ////////////////////////////////////////////////////
        // extract pod spec and add file /pod.yaml
        ////////////////////////////////////////////////////

        let pod_template_spec = spec.job_template.spec.unwrap_or_default().template;

        let mut pod = Pod::default();
        pod.spec = pod_template_spec.spec;
        pod.metadata = cron_job.metadata.clone();
        pod.metadata.name = Some(format!("crn-{}", ns_name.to_string()));
        let mut_spec = pod.spec.as_mut().unwrap();
        mut_spec.restart_policy = Some("Never".to_string());

        let pod_string = serde_yaml::to_string(&pod).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        self.store.write_file("cronjob", &ns_name.to_string(), "pod.yaml", pod_string.as_bytes())?;

        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true);
        ////////////////////////////////////////////////////
        // template cron-pod.service to /var/lib/state/store/cronjob/<name>/systemd.service
        ////////////////////////////////////////////////////


        handlebars.register_template_string("unit", include_str!("./resources/cron-pod.service")).map_err(|e| anyhow!(e).context("failed to load service template file"))?;

        let json: Value = json!({
            "description": &format!("{} Job", ns_name.to_string()),
            "timer": &format!("skate-cronjob-{}.timer", &ns_name.to_string()),
            "command": format!("podman kube play /var/lib/skate/store/cronjob/{}/pod.yaml --replace --network skate -w", ns_name.to_string()),
        });

        let output = handlebars.render("unit", &json)?;
        // /etc/systemd/system/skate-cronjob-{}.service

        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-cronjob-{}.service", &ns_name.to_string()))?;
        file.write_all(output.as_bytes())?;


        ////////////////////////////////////////////////////
        // template cron-pod.timer to /var/lib/state/store/cronjob/<name>/systemd.timer
        ////////////////////////////////////////////////////

        handlebars.register_template_string("timer", include_str!("./resources/cron-pod.timer")).map_err(|e| anyhow!(e).context("failed to load timer template file"))?;

        let json: Value = json!({
            "description": &format!("{} Timer", ns_name.to_string()),
            "target_unit": &format!("skate-cronjob-{}.service", &ns_name.to_string()),
            "on_calendar": systemd_timer_schedule,
        });

        let output = handlebars.render("timer", &json)?;
        // /etc/systemd/system/skate-cronjob-{}.timer
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-cronjob-{}.timer", &ns_name.to_string()))?;
        file.write_all(output.as_bytes())?;

        let unit_name = format!("skate-cronjob-{}", &ns_name.to_string());

        exec_cmd("systemctl", &["daemon-reload"])?;
        exec_cmd("systemctl", &["enable", "--now", &unit_name])?;
        let _ = exec_cmd("systemctl", &["reset-failed", &unit_name]);

        Ok(())
    }


    // TODO - warn about failures
    fn remove_cron(&self, cron: CronJob) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&cron);
        // systemctl stop skate-cronjob-{}
        let _ = exec_cmd("systemctl", &["stop", &format!("skate-cronjob-{}", &ns_name.to_string())]);

        // systemctl disable skate-cronjob-{}
        let _ = exec_cmd("systemctl", &["disable", &format!("skate-cronjob-{}", &ns_name.to_string())]);
        // rm /etc/systemd/system/skate-cronjob-{}.service
        let _ = exec_cmd("rm", &[&format!("/etc/systemd/system/skate-cronjob-{}.service", &ns_name.to_string())]);
        let _ = exec_cmd("rm", &[&format!("/etc/systemd/system/skate-cronjob-{}.timer", &ns_name.to_string())]);
        // systemctl daemon-reload
        let _ = exec_cmd("systemctl", &["daemon-reload"])?;
        // systemctl reset-failed
        let _ = exec_cmd("systemctl", &["reset-failed"])?;
        let _ = self.store.remove_object("cronjob", &ns_name.to_string())?;
        Ok(())
    }

    fn render_nginx_conf(&self) -> Result<(), Box<dyn Error>> {
        let le_allow_domains: Vec<_> = self.store.list_objects("ingress")?.into_iter().filter_map(|i| {
            match i.manifest {
                Some(m) => {
                    let rules = serde_yaml::from_value::<Ingress>(m).ok()?.spec?.rules?;
                    Some(rules.into_iter().filter_map(|r| r.host).collect::<Vec<String>>())
                }
                None => None
            }
        }).flatten().unique().collect();


        ////////////////////////////////////////////////////
        // Template main nginx conf
        ////////////////////////////////////////////////////

        let issuer = self.store.list_objects("clusterissuer").ok().and_then(|list| list.first().and_then(|c| Some(c.clone())));

        let (endpoint, email) = match issuer {
            Some(issuer) => {
                match serde_yaml::from_value::<ClusterIssuer>(issuer.manifest.clone().unwrap()).ok() {
                    Some(issuer) => Some((issuer.spec.clone().unwrap_or_default().acme.server.clone(), issuer.spec.unwrap_or_default().acme.email)),
                    None => None,
                }
            }
            None => None
        }.unwrap_or_default();

        let endpoint = if endpoint == "" {
            // default to staging
            "https://acme-staging-v02.api.letsencrypt.org/directory".to_string()
        } else {
            endpoint
        };

        let main_template_data = json!({
            "letsEncrypt": {
                "endpoint": endpoint, //
                "email": email,
                "allowDomains": le_allow_domains
            },
        });

        let child = process::Command::new("bash")
            .args(&["-c", "skatelet template --file /var/lib/skate/ingress/nginx.conf.tmpl - > /var/lib/skate/ingress/nginx.conf"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        child.stdin.as_ref().unwrap().write(main_template_data.to_string().as_ref()).unwrap();

        let output = child.wait_with_output()
            .map_err(|e| anyhow!(e).context("failed to apply resource"))?;

        if !output.status.success() {
            return Err(anyhow!("exit code {}, stderr: {}", output.status.code().unwrap(), String::from_utf8_lossy(&output.stderr).to_string()).into());
        }
        Ok(())
    }


    fn apply_ingress(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
        let ingress_string = serde_yaml::to_string(&ingress).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        let name = &metadata_name(&ingress).to_string();

        exec_cmd("mkdir", &["-p", &format!("/var/lib/skate/ingress/services/{}", name)])?;

        // manifest goes into store
        self.store.write_file("ingress", name, "manifest.yaml", ingress_string.as_bytes())?;

        let hash = ingress.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("ingress", &name, "hash", &hash.as_bytes())?;
        }

        self.render_nginx_conf()?;

        let _ns_name = metadata_name(&ingress);

        ////////////////////////////////////////////////////
        // Template service nginx confs for http/https
        ////////////////////////////////////////////////////

        for port in [80, 443] {
            // convert manifest to json
            // set "port" key
            let mut json_ingress = serde_json::to_value(&ingress).map_err(|e| anyhow!(e).context("failed to serialize manifest to json"))?;
            json_ingress["port"] = json!(port);

            let json_ingress_string = json_ingress.to_string();


            let child = process::Command::new("bash")
                .args(&["-c", &format!("skatelet template --file /var/lib/skate/ingress/service.conf.tmpl - > /var/lib/skate/ingress/services/{}/{}.conf", name, port)])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()).spawn()?;

            child.stdin.as_ref().unwrap().write(json_ingress_string.as_ref()).unwrap();

            let output = child.wait_with_output()
                .map_err(|e| anyhow!(e).context("failed to apply resource"))?;

            if !output.status.success() {
                return Err(anyhow!("exit code {}, stderr: {}", output.status.code().unwrap(), String::from_utf8_lossy(&output.stderr).to_string()).into());
            }
        }

        self.reload_ingress()?;

        Ok(())
    }

    fn remove_ingress(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&ingress);
        let _ = self.store.remove_object("ingress", &ns_name.to_string())?;
        let dir = format!("/var/lib/skate/ingress/services/{}", ns_name.to_string());
        let result = fs::remove_dir_all(&dir);
        if result.is_err() && result.as_ref().unwrap_err().kind() != std::io::ErrorKind::NotFound {
            return Err(anyhow!(result.unwrap_err()).context(format!("failed to remove directory {}", dir)).into());
        }

        self.reload_ingress()?;

        Ok(())
    }


    fn apply_play(&self, object: SupportedResources) -> Result<(), Box<dyn Error>> {
        let file_path = DefaultExecutor::write_manifest_to_file(&serde_yaml::to_string(&object)?)?;

        let mut args = vec!["play", "kube", &file_path, "--start"];
        if !object.host_network() {
            args.push("--network=skate")
        }

        let result = exec_cmd("podman", &args)?;

        if !result.is_empty() {
            println!("{}", result);
        }
        Ok(())
    }

    fn remove_secret(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        let fqn = format!("{}.{}", secret.metadata.name.unwrap(), secret.metadata.namespace.unwrap());
        let output = exec_cmd("podman", &["secret", "rm", &fqn])?;

        if !output.is_empty() {
            println!("{}", output);
        }

        Ok(())
    }

    fn apply_service(&self, service: Service) -> Result<(), Box<dyn Error>> {
        let manifest_string = serde_yaml::to_string(&service).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        let name = &metadata_name(&service).to_string();

        // manifest goes into store
        let yaml_path = self.store.write_file("service", name, "manifest.yaml", manifest_string.as_bytes())?;

        let hash = service.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("service", &name, "hash", &hash.as_bytes())?;
        }

        // install systemd service and timer
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(true);
        ////////////////////////////////////////////////////
        // template cron-pod.service to /var/lib/state/store/cronjob/<name>/systemd.service
        ////////////////////////////////////////////////////

        handlebars.register_template_string("unit", include_str!("./resources/skate-ipvsmon.service")).map_err(|e| anyhow!(e).context("failed to load service template file"))?;


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

        let json: Value = json!({
            "svc_name":name,
            "ip": ip,
            "yaml_path": yaml_path,
        });

        let file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-ipvsmon-{}.service", &name))?;
        handlebars.render_to_write("unit", &json, file)?;

        handlebars.register_template_string("timer", include_str!("./resources/skate-ipvsmon.timer")).map_err(|e| anyhow!(e).context("failed to load timer template file"))?;
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

    fn remove_service(&self, service: Service) -> Result<(), Box<dyn Error>> {
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


    fn remove_deployment(&self, deployment: Deployment, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        // find all pod ids for the deployment
        let name = deployment.metadata.name.unwrap();
        let ns = deployment.metadata.namespace.unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/deployment={}", name), "-q"])?;

        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        self.remove_pods(ids, grace_period)
    }

    fn remove_daemonset(&self, daemonset: DaemonSet, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let name = daemonset.metadata.name.unwrap();
        let ns = daemonset.metadata.namespace.unwrap_or("default".to_string());

        let ids = exec_cmd("podman", &["pod", "ls", "--filter", &format!("label=skate.io/namespace={}", ns), "--filter", &format!("label=skate.io/daemonset={}", name), "-q"])?;
        let ids = ids.split("\n").map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<&str>>();

        self.remove_pods(ids, grace_period)
    }

    fn remove_pods(&self, ids: Vec<&str>, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let failures: Vec<_> = ids.iter().filter_map(|id| {
            match self.remove_pod(id, grace_period) {
                Ok(_) => None,
                Err(e) => {
                    Some(e)
                }
            }
        }).collect();

        if !failures.is_empty() {
            let mut err = anyhow!("failures when removing pods");
            err = failures.iter().fold(err, |a, b| a.context(b.to_string()));
            return Err(err.into());
        }
        Ok(())
    }

    fn remove_pod(&self, id: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        if id.is_empty() {
            return Err(anyhow!("no metadata.name found").into());
        }

        let grace = grace_period.unwrap_or(10);

        let grace_str = format!("{}", grace);
        println!("gracefully stopping {}", id);

        let containers = exec_cmd("podman", &["pod", "inspect", &id, "--format={{range.Containers}}{{.Id}} {{end}}"])?;
        let containers = containers.split_ascii_whitespace().collect();

        let _ = exec_cmd("podman", &["pod", "kill", "--signal", "SIGTERM", &id]);


        let args = [vec!(&grace_str, "podman", "wait"), containers].concat();
        let result = exec_cmd("timeout", &args);

        if result.is_err() {
            eprintln!("failed to stop {}: {}", id, result.unwrap_err());
        }

        println!("removing {}", id);

        let rm_cmd = [
            vec!("pod", "rm", "--force"),
            vec!(&id),
        ].concat();

        let output = exec_cmd("podman", &rm_cmd)?;

        if !output.is_empty() {
            println!("{}", output);
        }

        Ok(())
    }

    pub(crate) fn apply_cluster_issuer(&self, cluster_issuer: ClusterIssuer) -> Result<(), Box<dyn Error>> {
        // only thing special about this is must only have namespace 'skate'
        // and name 'default'
        let ingress_string = serde_yaml::to_string(&cluster_issuer).map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(&cluster_issuer);
        // manifest goes into store
        self.store.write_file("clusterissuer", &ns_name.to_string(), "manifest.yaml", ingress_string.as_bytes())?;

        let hash = cluster_issuer.metadata.labels.as_ref().and_then(|m| m.get("skate.io/hash")).unwrap_or(&"".to_string()).to_string();

        if !hash.is_empty() {
            self.store.write_file("clusterissuer", &ns_name.to_string(), "hash", &hash.as_bytes())?;
        }
        // need to retemplate nginx.conf
        self.render_nginx_conf()?;

        self.reload_ingress()?;

        Ok(())
    }
    pub(crate) fn remove_cluster_issuer(&self, cluster_issuer: ClusterIssuer) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&cluster_issuer);


        let _ = self.store.remove_object("clusterissuer", &ns_name.to_string())?;

        // need to retemplate nginx.conf
        self.render_nginx_conf()?;
        self.reload_ingress()?;

        Ok(())
    }
}

impl Executor for DefaultExecutor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>> {
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        match object {
            SupportedResources::Pod(_)
            | SupportedResources::Secret(_)
            | SupportedResources::Deployment(_)
            | SupportedResources::DaemonSet(_) => {
                self.apply_play(object)
            }
            SupportedResources::Ingress(ingress) => {
                self.apply_ingress(ingress)
            }
            SupportedResources::CronJob(cron) => {
                self.apply_cronjob(cron)
            }
            SupportedResources::Service(service) => {
                self.apply_service(service)
            }
            SupportedResources::ClusterIssuer(issuer) => {
                self.apply_cluster_issuer(issuer)
            }
        }
    }


    fn manifest_delete(&self, object: SupportedResources, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        match object {
            SupportedResources::Pod(p) => {
                let name = p.metadata.name.unwrap();
                self.remove_pod(&name, grace_period)
            }
            SupportedResources::Deployment(d) => {
                self.remove_deployment(d, grace_period)
            }
            SupportedResources::DaemonSet(d) => {
                self.remove_daemonset(d, grace_period)
            }
            SupportedResources::Ingress(ingress) => {
                self.remove_ingress(ingress)
            }
            SupportedResources::CronJob(cron) => {
                self.remove_cron(cron)
            }
            SupportedResources::Secret(secret) => {
                self.remove_secret(secret)
            }
            SupportedResources::Service(service) => {
                self.remove_service(service)
            }
            SupportedResources::ClusterIssuer(issuer) => {
                self.remove_cluster_issuer(issuer)
            }
        }
    }
}
