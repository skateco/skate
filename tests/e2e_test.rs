use std::{env, panic, process};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{stderr, stdout};
use std::process::Stdio;
use serde::{Deserialize, Serialize};

fn setup() {}

fn teardown() {}

fn run_test<T>(test: T) -> ()
where
    T: FnOnce() -> () + panic::UnwindSafe,
{
    setup();
    let result = panic::catch_unwind(|| {
        test()
    });
    teardown();
    assert!(result.is_ok())
}

#[derive(Debug, Clone)]
struct SkateError {
    exit_code: i32,
    message: String,
}

impl Error for SkateError {}

impl Display for SkateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit code: {}, message: {}", self.exit_code, self.message)
    }
}

fn skate(command: &str, args: &[&str]) -> Result<(), SkateError> {
    let mut child = process::Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .stdout(stdout())
        .stderr(stderr())
        .spawn().map_err(|e| SkateError { exit_code: -1, message: e.to_string() })?;


    let status = child.wait().map_err(|e| SkateError{exit_code: -1, message: e.to_string()})?;
    if !status.success() {
        return Err(SkateError { exit_code: status.code().unwrap_or_default(), message:"".to_string() });
    }

    Ok(())
}

fn ips() -> Result<(String, String), anyhow::Error> {
    let output = process::Command::new("multipass")
        .args(&["info", "--format", "json"])
        .output()?;

    let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let node1 = info["info"]["node-1"]["ipv4"][0].as_str().ok_or(anyhow::anyhow!("failed to get node-1 ip"))?;
    let node2 = info["info"]["node-2"]["ipv4"][0].as_str().ok_or(anyhow::anyhow!("failed to get node-2 ip"))?;

    Ok((node1.to_string(), node2.to_string()))
}

#[test]
#[ignore]
fn test_cluster_creation() -> Result<(), anyhow::Error> {
    let ips = ips()?;

    let user = env::var("USER")?;

    skate("delete", &["cluster", "integration-test", "--yes"])?;
    skate("create", &["cluster", "integration-test"])?;
    skate("config", &["use-context", "integration-test"])?;
    skate("create", &["node", "--name", "node-1", "--host", &ips.0, "--subnet-cidr", "20.1.0.0/16", "--key", "/tmp/skate-e2e-key", "--user", &user])?;
    skate("create", &["node", "--name", "node-2", "--host", &ips.1, "--subnet-cidr", "20.2.0.0/16", "--key", "/tmp/skate-e2e-key", "--user", &user])?;

    // TODO -  validate that things work
    Ok(())
}