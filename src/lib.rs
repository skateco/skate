mod skate;
mod skatelet;
mod apply;
mod refresh;
mod ssh;
mod config;
mod scheduler;
mod util;
mod create;
mod delete;

mod state;
mod get;

mod describe;
mod filestore;
mod cron;
mod logs;
mod oci;
mod config_cmd;
mod spec;
mod template;
mod controllers;
mod cordon;

pub mod errors;
mod cluster;
mod rollout;
mod resource;
mod exec;
pub mod deps;
mod test_helpers;
mod upgrade;
mod github;
mod node_shell;

pub use skate::skate;
pub use skate::AllDeps;
pub use skatelet::skatelet;

use shadow_rs::shadow;
shadow!(build);
