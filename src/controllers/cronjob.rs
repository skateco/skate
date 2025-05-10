use crate::cron::cron_to_systemd;
use crate::errors::SkateError;
use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::skatelet::database::resource;
use crate::skatelet::database::resource::get_resource;
use crate::skatelet::VAR_PATH;
use crate::template;
use crate::util::metadata_name;
use anyhow::anyhow;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Pod;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::error::Error;
use std::fs;
use std::io::Write;

pub struct CronjobController {
    // TODO - get the pod spec from the db
    store: Box<dyn Store>,
    db: SqlitePool,
    execer: Box<dyn ShellExec>,
}

impl CronjobController {
    pub fn new(store: Box<dyn Store>, db: SqlitePool, execer: Box<dyn ShellExec>) -> Self {
        CronjobController { store, db, execer }
    }

    pub async fn apply(&self, cron_job: &CronJob) -> Result<(), Box<dyn Error>> {
        // let cron_job_string = serde_yaml::to_string(cron_job)
        //     .map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;

        let ns_name = metadata_name(cron_job);

        let hash = cron_job
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        let object = resource::Resource {
            name: ns_name.name.clone(),
            namespace: ns_name.namespace.clone(),
            resource_type: resource::ResourceType::CronJob,
            manifest: serde_json::to_value(cron_job)?,
            hash: hash.clone(),
            ..Default::default()
        };
        resource::insert_resource(&self.db, &object).await?;

        let spec = cron_job.spec.clone().unwrap_or_default();
        let timezone = spec.time_zone.unwrap_or_default();

        let systemd_timer_schedule = cron_to_systemd(&spec.schedule, &timezone)?;

        self.run(&ns_name.name, &ns_name.namespace, true).await?;

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

        self.execer.exec("systemctl", &["daemon-reload"], None)?;
        self.execer.exec(
            "systemctl",
            &["enable", &format!("{}.timer", &unit_name)],
            None,
        )?;
        self.execer.exec(
            "systemctl",
            &["start", &format!("{}.timer", &unit_name)],
            None,
        )?;
        let _ = self
            .execer
            .exec("systemctl", &["reset-failed", &unit_name], None);

        Ok(())
    }

    // TODO - warn about failures
    pub async fn delete(&self, cron: &CronJob) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(cron);
        let unit_name = format!("skate-cronjob-{}", &ns_name.to_string());
        // systemctl stop skate-cronjob-{}
        let _ = self.execer.exec("systemctl", &["stop", &unit_name], None);

        // systemctl disable skate-cronjob-{}
        let _ = self
            .execer
            .exec("systemctl", &["disable", &unit_name], None);
        // rm /etc/systemd/system/skate-cronjob-{}.service
        let _ = self.execer.exec(
            "rm",
            &[&format!(
                "/etc/systemd/system/skate-cronjob-{}.service",
                &ns_name.to_string()
            )],
            None,
        );
        let _ = self.execer.exec(
            "rm",
            &[&format!(
                "/etc/systemd/system/skate-cronjob-{}.timer",
                &ns_name.to_string()
            )],
            None,
        );
        // systemctl daemon-reload
        let _ = self.execer.exec("systemctl", &["daemon-reload"], None)?;
        // systemctl reset-failed
        let _ = self.execer.exec("systemctl", &["reset-failed"], None)?;

        // TODO - don't use file store for this
        let _ = self.store.remove_object("cronjob", &ns_name.to_string())?;

        resource::delete_resource(
            &self.db,
            &resource::ResourceType::CronJob,
            &ns_name.name,
            &ns_name.namespace,
        )
        .await?;

        Ok(())
    }

    pub async fn run(&self, name: &str, ns: &str, wait: bool) -> Result<(), SkateError> {
        let resource = get_resource(&self.db, &resource::ResourceType::CronJob, name, ns).await?;
        if resource.is_none() {
            return Err(anyhow!("failed to find cronjob").into());
        }
        let spec = resource.unwrap().manifest;
        let cronjob: CronJob = serde_json::from_value(spec)
            .map_err(|e| anyhow!(e).context("failed to deserialize manifest"))?;
        let spec = cronjob.spec.clone().unwrap_or_default();

        let pod_template_spec = spec.job_template.spec.unwrap_or_default().template;

        let mut pod = Pod {
            spec: pod_template_spec.spec,
            metadata: cronjob.metadata.clone(),
            ..Default::default()
        };

        pod.metadata.name = Some(format!("crn-{}.{}", name, ns));
        let mut_spec = pod.spec.as_mut().unwrap();
        mut_spec.restart_policy = Some("Never".to_string());

        let pod_string = serde_yaml::to_string(&pod)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to string"))?;

        // pod spec should be in VAR_PATH/cronjob/foo.bar/pod.yaml

        let args = &["kube", "play", "--replace", "--network", "skate", "-"];
        let args = if wait {
            [args.to_vec(), vec!["-w"]].concat()
        } else {
            args.to_vec()
        };

        self.execer.exec_stdout("podman", &args, Some(pod_string))?;
        Ok(())
    }
}
