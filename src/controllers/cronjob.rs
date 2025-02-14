use crate::cron::cron_to_systemd;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::template;
use crate::util::metadata_name;
use anyhow::anyhow;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Pod;
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::Write;

pub struct CronjobController {
    store: Box<dyn Store>,
    execer: Box<dyn ShellExec>,
}

impl CronjobController {
    pub fn new(store: Box<dyn Store>, execer: Box<dyn ShellExec>) -> Self {
        CronjobController { store, execer }
    }

    pub fn apply(&self, cron_job: &CronJob) -> Result<(), Box<dyn Error>> {
        let cron_job_string = serde_yaml::to_string(cron_job)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(cron_job);

        self.store.write_file(
            "cronjob",
            &ns_name.to_string(),
            "manifest.yaml",
            cron_job_string.as_bytes(),
        )?;

        let hash = cron_job
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        if !hash.is_empty() {
            self.store
                .write_file("cronjob", &ns_name.to_string(), "hash", hash.as_bytes())?;
        }

        let spec = cron_job.spec.clone().unwrap_or_default();
        let timezone = spec.time_zone.unwrap_or_default();

        let systemd_timer_schedule = cron_to_systemd(&spec.schedule, &timezone)?;

        ////////////////////////////////////////////////////
        // extract pod spec and add file /pod.yaml
        ////////////////////////////////////////////////////

        let pod_template_spec = spec.job_template.spec.unwrap_or_default().template;

        let mut pod = Pod {
            spec: pod_template_spec.spec,
            metadata: cron_job.metadata.clone(),
            ..Default::default()
        };

        pod.metadata.name = Some(format!("crn-{}", ns_name));
        let mut_spec = pod.spec.as_mut().unwrap();
        mut_spec.restart_policy = Some("Never".to_string());

        let pod_string = serde_yaml::to_string(&pod)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        let pod_yaml_path = self.store.write_file(
            "cronjob",
            &ns_name.to_string(),
            "pod.yaml",
            pod_string.as_bytes(),
        )?;

        // create the pod to test that it's valid
        self.execer
            .exec(
                "podman",
                &["kube", "play", "--start=false", "--replace", &pod_yaml_path],
            )
            .map_err(|e| anyhow!(e.to_string()).context("failed to create pod"))?;

        let mut handlebars = template::new();
        ////////////////////////////////////////////////////
        // template cron-pod.service to /var/lib/state/store/cronjob/<name>/systemd.service
        ////////////////////////////////////////////////////

        handlebars
            .register_template_string("unit", include_str!("../resources/cron-pod.service"))
            .map_err(|e| anyhow!(e).context("failed to load service template file"))?;

        let unit_name = format!("skate-cronjob-{}", &ns_name.to_string());

        let json: Value = json!({
            "description": &format!("{} Job", ns_name),
            "timer": &format!("{}.timer", &unit_name),
            "command": format!("skatelet create --namespace {} job --from cronjob/{} {} -w", ns_name.namespace, ns_name.name, ns_name.name),
        });

        let output = handlebars.render("unit", &json)?;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!("/etc/systemd/system/{}.service", &unit_name))?;
        file.write_all(output.as_bytes())?;

        ////////////////////////////////////////////////////
        // template cron-pod.timer to /var/lib/state/store/cronjob/<name>/systemd.timer
        ////////////////////////////////////////////////////

        handlebars
            .register_template_string("timer", include_str!("../resources/cron-pod.timer"))
            .map_err(|e| anyhow!(e).context("failed to load timer template file"))?;

        let json: Value = json!({
            "description": &format!("{} Timer", ns_name),
            "target_unit": &format!("{}.service", &unit_name),
            "on_calendar": systemd_timer_schedule,
        });

        let output = handlebars.render("timer", &json)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(format!("/etc/systemd/system/{}.timer", &unit_name))?;
        file.write_all(output.as_bytes())?;

        self.execer.exec("systemctl", &["daemon-reload"])?;
        self.execer
            .exec("systemctl", &["enable", &format!("{}.timer", &unit_name)])?;
        self.execer
            .exec("systemctl", &["start", &format!("{}.timer", &unit_name)])?;
        let _ = self.execer.exec("systemctl", &["reset-failed", &unit_name]);

        Ok(())
    }

    // TODO - warn about failures
    pub fn delete(&self, cron: &CronJob) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(cron);
        let unit_name = format!("skate-cronjob-{}", &ns_name.to_string());
        // systemctl stop skate-cronjob-{}
        let _ = self.execer.exec("systemctl", &["stop", &unit_name]);

        // systemctl disable skate-cronjob-{}
        let _ = self.execer.exec("systemctl", &["disable", &unit_name]);
        // rm /etc/systemd/system/skate-cronjob-{}.service
        let _ = self.execer.exec(
            "rm",
            &[&format!(
                "/etc/systemd/system/skate-cronjob-{}.service",
                &ns_name.to_string()
            )],
        );
        let _ = self.execer.exec(
            "rm",
            &[&format!(
                "/etc/systemd/system/skate-cronjob-{}.timer",
                &ns_name.to_string()
            )],
        );
        // systemctl daemon-reload
        let _ = self.execer.exec("systemctl", &["daemon-reload"])?;
        // systemctl reset-failed
        let _ = self.execer.exec("systemctl", &["reset-failed"])?;
        let _ = self.store.remove_object("cronjob", &ns_name.to_string())?;
        Ok(())
    }

    pub fn run(&self, name: &str, ns: &str, wait: bool) -> Result<(), SkateError> {
        let obj = self
            .store
            .get_object("cronjob", &format!("{}.{}", name, ns))?;

        let args = &[
            "kube",
            "play",
            &format!("{}/pod.yaml", obj.path),
            "--replace",
            "--network",
            "skate",
        ];
        let args = if wait {
            [args.to_vec(), vec!["-w"]].concat()
        } else {
            args.to_vec()
        };

        self.execer.exec_stdout("podman", &args)?;
        Ok(())
    }
}
