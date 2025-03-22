use crate::exec::ShellExec;
use crate::filestore::Store;
use crate::skatelet::VAR_PATH;
use crate::spec::cert::ClusterIssuer;
use crate::util::metadata_name;
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::networking::v1::Ingress;
use serde_json::json;
use std::error::Error;
use std::io::Write;
use std::process::Stdio;
use std::{fs, process};

pub struct IngressController {
    store: Box<dyn Store>,
    execer: Box<dyn ShellExec>,
    ingress_path: String,
}

impl IngressController {
    pub fn new(store: Box<dyn Store>, execer: Box<dyn ShellExec>) -> Self {
        IngressController {
            store,
            execer,
            ingress_path: format!("{}/ingress", VAR_PATH),
        }
    }

    pub fn apply(&self, ingress: &Ingress) -> Result<(), Box<dyn Error>> {
        let ingress_string = serde_yaml::to_string(ingress)
            .map_err(|e| anyhow!(e).context("failed to serialize manifest to yaml"))?;
        let name = &metadata_name(ingress).to_string();

        self.execer.exec(
            "mkdir",
            &["-p", &format!("{}/services/{}", self.ingress_path, name)],
        )?;

        // manifest goes into store
        self.store
            .write_file("ingress", name, "manifest.yaml", ingress_string.as_bytes())?;

        let hash = ingress
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get("skate.io/hash"))
            .unwrap_or(&"".to_string())
            .to_string();

        if !hash.is_empty() {
            self.store
                .write_file("ingress", name, "hash", hash.as_bytes())?;
        }

        self.render_nginx_conf()?;

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

    pub fn delete(&self, ingress: &Ingress) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(ingress);
        let dir = format!("{}/services/{}", self.ingress_path, ns_name);
        let result = fs::remove_dir_all(&dir);
        if result.is_err() && result.as_ref().unwrap_err().kind() != std::io::ErrorKind::NotFound {
            return Err(anyhow!(result.unwrap_err())
                .context(format!("failed to remove directory {}", dir))
                .into());
        }

        self.store.remove_object("ingress", &ns_name.to_string())?;

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
        )?;

        if id.is_empty() {
            return Err(anyhow!("no ingress container found").into());
        }

        let _ = self
            .execer
            .exec("podman", &["kill", "--signal", "HUP", &id.to_string()])?;
        Ok(())
    }

    pub fn render_nginx_conf(&self) -> Result<(), Box<dyn Error>> {
        let le_allow_domains: Vec<_> = self
            .store
            .list_objects("ingress")?
            .into_iter()
            .filter_map(|i| match i.manifest {
                Some(m) => {
                    let rules = serde_yaml::from_value::<Ingress>(m).ok()?.spec?.rules?;
                    Some(
                        rules
                            .into_iter()
                            .filter_map(|r| r.host)
                            .collect::<Vec<String>>(),
                    )
                }
                None => None,
            })
            .flatten()
            .unique()
            .collect();

        ////////////////////////////////////////////////////
        // Template main nginx conf
        ////////////////////////////////////////////////////

        let issuer = self
            .store
            .list_objects("clusterissuer")
            .ok()
            .and_then(|list| list.first().cloned());

        let (endpoint, email) = match issuer {
            Some(issuer) => {
                match serde_yaml::from_value::<ClusterIssuer>(issuer.manifest.clone().unwrap()).ok()
                {
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
