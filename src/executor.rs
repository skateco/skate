use std::error::Error;
use std::fs::File;
use std::io::{Write};
use std::process;
use std::process::Stdio;

use anyhow::anyhow;
use handlebars::Handlebars;

use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Pod, Secret};
use k8s_openapi::api::networking::v1::Ingress;
use serde_json::{json, Value};

use crate::cron::cron_to_systemd;
use crate::filestore::FileStore;
use crate::skate::{exec_cmd, SupportedResources};
use crate::util::{hash_string, metadata_name};

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
        // extract pod spec and add file /pod-manifest.yaml
        ////////////////////////////////////////////////////

        let pod_template_spec = spec.job_template.spec.unwrap_or_default().template;

        let mut pod = Pod::default();
        pod.spec = pod_template_spec.spec;
        pod.metadata = cron_job.metadata.clone();
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
            "command": format!("podman kube play /var/lib/skate/store/cronjob/{}/pod.yaml --replace --network podman -w", ns_name.to_string()),
        });

        let output = handlebars.render("unit", &json)?;
        // /etc/systemd/system/skate-cronjob-{}.service

        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-cronjob-{}.service", &ns_name.to_string()))?;
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
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&format!("/etc/systemd/system/skate-cronjob-{}.timer", &ns_name.to_string()))?;
        file.write_all(output.as_bytes())?;


        // systemctl daemon-reload
        exec_cmd("systemctl", &["daemon-reload"])?;
        exec_cmd("systemctl", &["enable", "--now", &format!("skate-cronjob-{}", &ns_name.to_string())])?;

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


        ////////////////////////////////////////////////////
        // Template main nginx conf
        ////////////////////////////////////////////////////

        let main_template_data = json!({
            "letsEncrypt": {
                "endpoint": ""
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
        let result = std::fs::remove_dir_all(&dir);
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
            args.push("--network=podman")
        }

        let output = process::Command::new("podman")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect(&format!("failed to apply resource via `podman {}`", &args.join(" ")));

        if !output.status.success() {
            return Err(anyhow!("`podman {}` exited with code {}, stderr: {}", args.join(" "), output.status.code().unwrap(), String::from_utf8_lossy(&output.stderr).to_string()).into());
        }


        Ok(())
    }

    fn remove_secret(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        let fqn = format!("{}.{}", secret.metadata.name.unwrap(), secret.metadata.namespace.unwrap());

        let output = process::Command::new("podman")
            .args(["secret", "rm", &fqn])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to remove secret");

        if !output.status.success() {
            return Err(anyhow!("`podman secret rm {}` exited with code {}, stderr: {}", fqn, output.status.code().unwrap(), String::from_utf8_lossy(&output.stderr).trim().to_string()).into());
        }

        if !output.stdout.is_empty() {
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }

        Ok(())
    }

    fn remove_pod(&self, id: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        if id.is_empty() {
            return Err(anyhow!("no metadata.name found").into());
        }

        let grace = grace_period.unwrap_or(10);

        let grace_str = format!("{}", grace);
        let stop_cmd = [
            vec!("pod", "stop", "-t", &grace_str),
            vec!(&id),
        ].concat();
        let _output = process::Command::new("podman")
            .args(stop_cmd.clone())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to stop pod");

        // if !output.status.success() {
        //     return Err(anyhow!("{:?} - exit code {}, stderr: {}", stop_cmd, output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        // }

        let rm_cmd = [
            vec!("pod", "rm", "--force"),
            vec!(&id),
        ].concat();
        let output = process::Command::new("podman")
            .args(rm_cmd.clone())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to remove pod");

        if !output.status.success() {
            return Err(anyhow!("{:?} - exit code {}, stderr: {}", rm_cmd,  output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        }
        Ok(())
    }
}

impl Executor for DefaultExecutor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>> {
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        match object {
            SupportedResources::Pod(_) | SupportedResources::Deployment(_) | SupportedResources::DaemonSet(_) | SupportedResources::Secret(_) => {
                self.apply_play(object)
            }
            SupportedResources::Ingress(ingress) => {
                self.apply_ingress(ingress)
            }
            SupportedResources::CronJob(cron) => {
                self.apply_cronjob(cron)
            }
        }
    }


    fn manifest_delete(&self, object: SupportedResources, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        match object {
            SupportedResources::Pod(p) => {
                let name = p.metadata.name.unwrap();
                self.remove_pod(&name, grace_period)
            }
            SupportedResources::Deployment(_d) => {
                Err(anyhow!("removing a deployment is not supported, instead supply it's individual pods").into())
            }
            SupportedResources::DaemonSet(_) => {
                Err(anyhow!("removing a daemonset is not supported, instead supply it's individual pods").into())
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
        }
    }
}
