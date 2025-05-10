use crate::exec::ShellExec;
use crate::supported_resources::SupportedResources;
use crate::util::{apply_play, metadata_name};
use k8s_openapi::api::core::v1::Secret;
use std::error::Error;

pub struct SecretController {
    execer: Box<dyn ShellExec>,
}

impl SecretController {
    pub fn new(execer: Box<dyn ShellExec>) -> Self {
        SecretController { execer }
    }

    pub fn apply(&self, secret: &Secret) -> Result<(), Box<dyn Error>> {
        apply_play(&self.execer, &SupportedResources::Secret(secret.clone()))
    }

    pub fn delete(&self, secret: &Secret) -> Result<(), Box<dyn Error>> {
        let name = metadata_name(secret);
        let output = self
            .execer
            .exec("podman", &["secret", "rm", &name.to_string()], None)?;

        if !output.is_empty() {
            println!("{}", output);
        }

        Ok(())
    }
}
