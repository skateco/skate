use crate::skate::exec_cmd;
use crate::util::{apply_play, metadata_name};
use k8s_openapi::api::core::v1::Secret;
use std::error::Error;
use crate::resource::SupportedResources;

pub struct SecretController {
}

impl SecretController {
    pub fn new() -> Self {
        SecretController {
        }
    }

    pub fn apply(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        apply_play(SupportedResources::Secret(secret))
    }


    pub fn delete(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        let name = metadata_name(&secret);
        let output = exec_cmd("podman", &["secret", "rm", &name.to_string()])?;

        if !output.is_empty() {
            println!("{}", output);
        }

        Ok(())
    }
}