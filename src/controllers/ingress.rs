use std::error::Error;
use std::io::Write;
use std::{fs, process};
use std::process::Stdio;
use anyhow::anyhow;
use itertools::Itertools;
use k8s_openapi::api::networking::v1::Ingress;
use serde_json::json;
use crate::filestore::FileStore;
use crate::skate::exec_cmd;
use crate::spec::cert::ClusterIssuer;
use crate::util::metadata_name;

pub struct IngressController {
    store: FileStore
}

impl IngressController {
    pub fn new(file_store: FileStore) -> Self {
        IngressController {
            store: file_store
        }
    }

    pub fn apply(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
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

        Self::reload()?;

        Ok(())
    }

    pub fn delete(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
        let ns_name = metadata_name(&ingress);
        let _ = self.store.remove_object("ingress", &ns_name.to_string())?;
        let dir = format!("/var/lib/skate/ingress/services/{}", ns_name.to_string());
        let result = fs::remove_file(&dir);
        if result.is_err() && result.as_ref().unwrap_err().kind() != std::io::ErrorKind::NotFound {
            return Err(anyhow!(result.unwrap_err()).context(format!("failed to remove directory {}", dir)).into());
        }

        Self::reload()?;

        Ok(())
    }

    pub fn reload() -> Result<(), Box<dyn Error>> {

        // trigger SIGHUP to ingress container
        // sudo bash -c "podman kill --signal HUP \$(podman ps --filter label=skate.io/namespace=skate --filter label=skate.io/daemonset=nginx-ingress -q)"
        let id = exec_cmd("podman", &["ps", "--filter", "label=skate.io/namespace=skate", "--filter", "label=skate.io/daemonset=nginx-ingress", "-q"])?;

        if id.is_empty() {
            return Err(anyhow!("no ingress container found").into());
        }

        let _ = exec_cmd("podman", &["kill", "--signal", "HUP", &format!("{}", id)])?;
        Ok(())
    }

    pub fn render_nginx_conf(&self) -> Result<(), Box<dyn Error>> {
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
}

