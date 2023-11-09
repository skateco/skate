use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::process;
use std::process::Stdio;
use anyhow::anyhow;
use crate::skate::SupportedResources;
use crate::util::hash_string;

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
        let file_path = DefaultExecutor::write_to_file(manifest)?;

        let output = process::Command::new("podman")
            .args(["play", "kube", &file_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()

            .expect("failed to apply resource");
        if !output.status.success() {
            return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        }
        Ok(())
    }

    fn remove(&self, manifest: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let object: SupportedResources = serde_yaml::from_str(manifest).expect("failed to deserialize manifest");
        let id = match object {
            SupportedResources::Pod(p) => p.metadata.name.unwrap_or("".to_string()),
            SupportedResources::Deployment(d) => d.metadata.name.unwrap_or("".to_string())
        };

        println!("id {}", id);

        let grace = grace_period.unwrap_or(10);

        let output = process::Command::new("podman")
            .args(["pod", "stop", &id, "-t", &format!("{}", grace)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to stop pod");

        if !output.status.success() {
            return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        }
        let output = process::Command::new("podman")
            .args(["pod", "rm", &id])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("failed to remove pod");

        if !output.status.success() {
            return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).into());
        }
        Ok(())
    }
}
