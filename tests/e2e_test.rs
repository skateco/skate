use std::{panic, process};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

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

fn skate(command: &str, args: &[&str]) -> Result<String, SkateError> {
    let output = process::Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .output().map_err(|e| SkateError { exit_code: -1, message: e.to_string() })?;
    if !output.status.success() {
        return Err(SkateError { exit_code: output.status.code().unwrap_or_default(), message: String::from_utf8_lossy(&output.stderr).to_string() });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}

fn ips() -> Result<(String, String), anyhow::Error> {
    let output = process::Command::new("../hack/clusterplz")
        .args(&["ips"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    Ok((lines.nth(0).unwrap().into(), lines.nth(0).unwrap().into()))
}

#[test]
#[ignore]
fn test_node_creation() -> Result<(), anyhow::Error> {
    let ips = ips()?;
    skate("delete", &["cluster", "integration-test", "--force"])?;
    skate("create", &["cluster", "integration-test"])?;
    skate("config", &["use-context", "integration-test"])?;
     skate("create", &["node", "--name", "node-1", "--host", "192.168.1.99", "--subnet-cidr", "20.1.0.0/16"])?;
    Ok(())
}