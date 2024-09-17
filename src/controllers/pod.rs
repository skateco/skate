use std::error::Error;
use anyhow::anyhow;
use k8s_openapi::api::core::v1::Pod;
use crate::filestore::FileStore;
use crate::skate::{exec_cmd, SupportedResources};
use crate::util::apply_play;

pub struct PodController {
}

impl PodController {
    pub fn new() -> Self {
        PodController {
        }
    }

    pub fn apply(&self, pod: Pod) -> Result<(), Box<dyn Error>> {
        apply_play(SupportedResources::Pod(pod))
    }

    pub fn delete(&self, pod: Pod, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let name = pod.metadata.name.unwrap();
        self.delete_podman_pod(&name, grace_period)
    }

    pub fn delete_podman_pods(&self, ids: Vec<&str>, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
        let failures: Vec<_> = ids.iter().filter_map(|id| {
            match self.delete_podman_pod(id, grace_period) {
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

    fn delete_podman_pod(&self, id: &str, grace_period: Option<usize>) -> Result<(), Box<dyn Error>> {
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
}
