use std::error::Error;
use k8s_openapi::api::core::v1::Secret;
use crate::filestore::FileStore;
use crate::skate::{exec_cmd, SupportedResources};
use crate::util::apply_play;

pub struct SecretController {
    store: FileStore,
}

impl SecretController {
    pub fn new(file_store: FileStore) -> Self {
        SecretController {
            store: file_store,
        }
    }

    pub fn apply(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        apply_play(SupportedResources::Secret(secret))
    }


    pub fn delete(&self, secret: Secret) -> Result<(), Box<dyn Error>> {
        let fqn = format!("{}.{}", secret.metadata.name.unwrap(), secret.metadata.namespace.unwrap());
        let output = exec_cmd("podman", &["secret", "rm", &fqn])?;

        if !output.is_empty() {
            println!("{}", output);
        }

        Ok(())
    }
}