use std::error::Error;
use std::fs::File;
use std::io::{Write};
use std::process;
use std::process::Stdio;
use anyhow::anyhow;
use k8s_openapi::api::networking::v1::Ingress;
use serde_json::json;
use serde_json::Value::Number;
use crate::skate::{exec_cmd, SupportedResources};
use crate::util::{hash_string, metadata_name};

pub trait Executor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>>;
    fn remove(&self, manifest: &str, grace: Option<usize>) -> Result<(), Box<dyn Error>>;
}

pub struct DefaultExecutor {}

impl DefaultExecutor {
    fn write_to_file(manifest: &str) -> Result<String, Box<dyn Error>> {
        let file_path = format!("/tmp/skate-{}.yaml", hash_string(manifest));
        let mut file = File::create(file_path.clone()).expect("failed to open file for manifests");
        file.write_all(manifest.as_ref()).expect("failed to write manifest to file");
        Ok(file_path)
    }

    fn apply_ingress(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
        let output = exec_cmd("mkdir", &["-p", "/var/lib/skate/ingress/services"])?;

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

        let ns_name = metadata_name(&ingress);

        for port in [80, 443] {
            // convert manifest to json
            // set "port" key
            let mut json_ingress = serde_json::to_value(&ingress).map_err(|e| anyhow!(e).context("failed to serialize manifest to json"))?;
            json_ingress["port"] = json!(port);

            let json_ingress_string = json_ingress.to_string();


            let mut child = process::Command::new("bash")
                .args(&["-c", &format!("skatelet template --file /var/lib/skate/ingress/service.conf.tmpl - > /var/lib/skate/ingress/services/ingress--{}--{}.conf", ns_name.name, port)])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()).spawn()?;

            child.stdin.as_ref().unwrap().write(json_ingress_string.as_ref()).unwrap();


            let output = child.wait_with_output()
                .map_err(|e| anyhow!(e).context("failed to apply resource"))?;


            if !output.status.success() {
                return Err(anyhow!("exit code {}, stderr: {}", output.status.code().unwrap(), String::from_utf8_lossy(&output.stderr).to_string()).into());
            }
        }


        // trigger SIGHUP to ingress container
        // sudo bash -c "podman kill --signal HUP \$(podman ps --filter label=skate.io/namespace=skate --filter label=skate.io/daemonset=nginx-ingress -q)"
        let id = exec_cmd("podman", &["ps", "--filter", "label=skate.io/namespace=skate", "--filter", "label=skate.io/daemonset=nginx-ingress", "-q"])?;

        if id.is_empty() {
            return Err(anyhow!("no ingress container found").into());
        }

        exec_cmd("podman", &["kill", "--signal", "HUP", &format!("{}", id)])?;

        Ok(())
    }

    fn remove_ingress(&self, ingress: Ingress) -> Result<(), Box<dyn Error>> {
        // in all nodes
        // in /etc/skate/ingress/includes delete any <ingress-name>--<namespace>.conf
        // trigger SIGHUP to ingress container
        Ok(())
    }

    fn apply_play(&self, object: SupportedResources) -> Result<(), Box<dyn Error>> {

        // check if object's hostNetwork: true then don't use network=podman

        let file_path = DefaultExecutor::write_to_file(&serde_yaml::to_string(&object)?)?;

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
    fn remove_pod(&self, id: &str, namespace: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
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
            SupportedResources::Pod(_) | SupportedResources::Deployment(_) | SupportedResources::DaemonSet(_) => {
                self.apply_play(object)
            }
            SupportedResources::Ingress(ingress) => {
                self.apply_ingress(ingress)
            }
        }
    }

    fn remove(&self, manifest: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");

        let namespaced_name = object.name();
        match object {
            SupportedResources::Pod(p) => {
                self.remove_pod(&namespaced_name.name, &namespaced_name.namespace, grace_period)
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
        }
    }
}
