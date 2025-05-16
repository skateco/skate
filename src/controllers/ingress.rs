use crate::exec::ShellExec;
use crate::skatelet::database::resource;
use crate::skatelet::database::resource::{
    delete_resource, list_resources_by_type, upsert_resource, ResourceType,
};
use crate::skatelet::VAR_PATH;
use crate::spec::cert::ClusterIssuer;
use crate::util::{get_skate_label_value, metadata_name, SkateLabels};
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::networking::v1::Ingress;
use serde_json::json;
use sqlx::SqlitePool;
use std::error::Error;
use std::io::Write;
use std::process::Stdio;
use std::{fs, process};

pub struct IngressController {
    db: SqlitePool,
    execer: Box<dyn ShellExec>,
    ingress_path: String,
}

impl IngressController {
    pub fn new(db: SqlitePool, execer: Box<dyn ShellExec>) -> Self {
        IngressController {
            db,
            execer,
            ingress_path: format!("{}/ingress", VAR_PATH),
        }
    }

    pub async fn apply(&self, ingress: &Ingress) -> Result<(), Box<dyn Error>> {
        let fq_name = metadata_name(ingress);
        let name = fq_name.to_string();

        self.execer.exec(
            "mkdir",
            &["-p", &format!("{}/services/{}", self.ingress_path, name)],
            None,
        )?;

        let hash = get_skate_label_value(&ingress.metadata.labels, &SkateLabels::Hash)
            .unwrap_or("".to_string());
        let generation = ingress.metadata.generation.unwrap_or_default();

        let object = resource::Resource {
            name: fq_name.name,
            namespace: fq_name.namespace,
            resource_type: resource::ResourceType::Ingress,
            manifest: serde_json::to_value(ingress)?,
            hash: hash.clone(),
            generation,
            ..Default::default()
        };

        upsert_resource(&self.db, &object).await?;

        self.render_nginx_conf().await?;

        let _ns_name = metadata_name(ingress);

        ////////////////////////////////////////////////////
        // Template service nginx confs for http/https
        ////////////////////////////////////////////////////

        for port in [80, 443] {
            // convert manifest to json
            // set "port" key
            let mut json_ingress = serde_json::to_value(ingress)
                .map_err(|e| anyhow!(e).context("failed to serialize manifest to json"))?;
            json_ingress["port"] = json!(port);

            let json_ingress_string = json_ingress.to_string();
            let ingress_path = &self.ingress_path;

            let child = process::Command::new("bash")
                .args([
                    "-c",
                    &format!(
                        "skatelet template --file {ingress_path}/service.conf.tmpl - > {ingress_path}/services/{name}/{port}.conf",
                    ),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;

            let _ = child
                .stdin
                .as_ref()
                .unwrap()
                .write(json_ingress_string.as_ref())
                .unwrap();

            let output = child
                .wait_with_output()
                .map_err(|e| anyhow!(e).context("failed to apply resource"))?;

            if !output.status.success() {
                return Err(anyhow!(
                    "exit code {}, stderr: {}",
                    output.status.code().unwrap(),
                    String::from_utf8_lossy(&output.stderr).to_string()
                )
                .into());
            }
        }

        self.reload()?;

        Ok(())
    }

    pub async fn delete(&self, ingress: &Ingress) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(ingress);
        let dir = format!("{}/services/{}", self.ingress_path, ns_name);
        let result = fs::remove_dir_all(&dir);
        if result.is_err() && result.as_ref().unwrap_err().kind() != std::io::ErrorKind::NotFound {
            return Err(anyhow!(result.unwrap_err())
                .context(format!("failed to remove directory {}", dir))
                .into());
        }

        delete_resource(
            &self.db,
            &ResourceType::Ingress,
            &ns_name.name,
            &ns_name.namespace,
        )
        .await?;

        self.reload()?;

        Ok(())
    }

    pub fn reload(&self) -> Result<(), Box<dyn Error>> {
        // trigger SIGHUP to ingress container
        // sudo bash -c "podman kill --signal HUP \$(podman ps --filter label=skate.io/namespace=skate --filter label=skate.io/daemonset=nginx-ingress -q)"
        let id = self.execer.exec(
            "podman",
            &[
                "ps",
                "--filter",
                "label=skate.io/namespace=skate",
                "--filter",
                "label=skate.io/daemonset=nginx-ingress",
                "-q",
            ],
            None,
        )?;

        if id.is_empty() {
            return Err(anyhow!("no ingress container found").into());
        }

        let _ = self.execer.exec(
            "podman",
            &["kill", "--signal", "HUP", &id.to_string()],
            None,
        )?;
        Ok(())
    }

    pub async fn render_nginx_conf(&self) -> Result<(), Box<dyn Error>> {
        let ingresses = list_resources_by_type(&self.db, &ResourceType::Ingress).await?;

        let le_allow_domains: Vec<_> = ingresses
            .into_iter()
            .filter_map(|i| {
                let ingress = serde_json::from_value::<Ingress>(i.manifest).ok();
                match ingress {
                    Some(ingress) => {
                        let rules = ingress.spec?.rules?;
                        Some(
                            rules
                                .into_iter()
                                .filter_map(|r| r.host)
                                .collect::<Vec<String>>(),
                        )
                    }
                    None => None,
                }
            })
            .flatten()
            .unique()
            .collect();

        ////////////////////////////////////////////////////
        // Template main nginx conf
        ////////////////////////////////////////////////////

        let cluster_issuers = list_resources_by_type(&self.db, &ResourceType::ClusterIssuer)
            .await
            .ok();

        let issuer = cluster_issuers.and_then(|list| list.first().cloned());

        let (endpoint, email) = match issuer {
            Some(issuer) => {
                match serde_json::from_value::<ClusterIssuer>(issuer.manifest.clone()).ok() {
                    Some(issuer) => Some((
                        issuer.spec.clone().unwrap_or_default().acme.server.clone(),
                        issuer.spec.unwrap_or_default().acme.email,
                    )),
                    None => None,
                }
            }
            None => None,
        }
        .unwrap_or_default();

        let endpoint = if endpoint.is_empty() {
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
        let ingress_path = &self.ingress_path;

        let child = process::Command::new("bash")
            .args(["-c", &format!("skatelet template --file {ingress_path}/nginx.conf.tmpl - > {ingress_path}/nginx.conf")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let _ = child
            .stdin
            .as_ref()
            .unwrap()
            .write(main_template_data.to_string().as_ref())
            .unwrap();

        let output = child
            .wait_with_output()
            .map_err(|e| anyhow!(e).context("failed to apply resource"))?;

        if !output.status.success() {
            return Err(anyhow!(
                "exit code {}, stderr: {}",
                output.status.code().unwrap(),
                String::from_utf8_lossy(&output.stderr).to_string()
            )
            .into());
        }
        Ok(())
    }
}
