use std::error::Error;
use std::fs::File;
use std::io::{Write};
use std::process;
use std::process::Stdio;
use anyhow::anyhow;
use crate::skate::SupportedResources;
use crate::util::{hash_string};

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
}

impl Executor for DefaultExecutor {
    fn apply(&self, manifest: &str) -> Result<(), Box<dyn Error>> {
        // just to check
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");

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

        println!("{}", String::from_utf8_lossy(&output.stdout).to_string());

        Ok(())
    }

    fn remove(&self, manifest: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        let (id, ns) = match object {
            SupportedResources::Pod(p) => {
                (p.metadata.name.unwrap_or("".to_string()),
                 p.metadata.namespace.unwrap_or("".to_string()))
            }
            SupportedResources::Deployment(_d) => {
                return Err(anyhow!("removing a deployment is not supported, instead supply it's individual pods").into());
            }
            SupportedResources::DaemonSet(_) => {
                todo!("remove daemonset")
            }
        };
        let id = id.trim().to_string();
        let ns = ns.trim().to_string();

        if id.is_empty() {
            return Err(anyhow!("no metadata.name found").into());
        }
        if ns.is_empty() {
            return Err(anyhow!("no metadata.name found").into());
        }

        let grace = grace_period.unwrap_or(10);

        let grace_str = format!("{}", grace);
        let stop_cmd = [
            vec!("pod", "stop", "-t", &grace_str),
            vec!(&id),
        ].concat();
        let output = process::Command::new("podman")
            .args(stop_cmd.clone())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to stop pod");

        if !output.status.success() {
            return Err(anyhow!("{:?} - exit code {}, stderr: {}", stop_cmd, output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        }

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
