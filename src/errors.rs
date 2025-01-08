use thiserror::Error;
use std::error::Error as RustError;
use handlebars::RenderError;
use validator::{ValidationErrors};
use crate::ssh::SshError;

#[derive(Error, Debug)]
pub enum SkateError {
    #[error("Error: {0}")]
    String(String),
    #[error("Error: {0}")]
    Anyhow(#[from] anyhow::Error),
    #[error("Error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Error: {0}")]
    Syslog(#[from] syslog::Error),
    #[error("Error: {0}")]
    Boxed(#[from]Box<dyn RustError>),
    #[error("Error: {0}")]
    Render(#[from] RenderError),
    #[error("Error: {0}")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("Error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Error: {0}")]
    Ssh(#[from] SshError),
    #[error("Error: {0:?}")]
    Multi(Vec<SkateError>),
    #[error("Error: {}", .0)]
    ValidationErrors(#[from] ValidationErrors),
    #[error("unknown error")]
    Unknown,
}

impl From<String> for SkateError {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
