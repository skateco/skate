mod apply;
mod config;
mod create;
mod delete;
mod refresh;
mod scheduler;
mod skate;
mod skatelet;
mod ssh;
mod util;

mod get;
mod state;

mod config_cmd;
mod controllers;
mod cordon;
mod cron;
mod describe;
mod filestore;
mod logs;
mod oci;
mod spec;
mod template;

mod cluster;
pub mod deps;
pub mod errors;
mod exec;
mod github;
mod http_client;
mod node_shell;
mod rollout;
pub mod sind;
pub(crate) mod supported_resources;
mod test_helpers;
mod upgrade;

pub use skate::skate;
pub use skate::AllDeps;
pub use skatelet::skatelet;

use shadow_rs::shadow;
shadow!(build);
