use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct When {
    #[serde(rename = "hasBindMounts")]
    pub has_bind_mounts: Option<bool>,
    pub annotations: Option<HashMap<String,String>>,
    pub always: Option<bool>,
    pub commands: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Command {
    pub path: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum Stage {
    #[serde(rename = "prestart")]
    PreStart,
    #[serde(rename = "poststart")]
    PostStart,
    #[serde(rename = "poststop")]
    PostStop,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct HookConfig {
    pub version: String,
    pub hook: Command,
    pub when: When,
    pub stages: Vec<Stage>,
}