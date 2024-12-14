use std::error::Error;
use std::{panic, process};
use std::fmt::{Debug, Formatter};
use anyhow::anyhow;

fn setup() {

}

fn teardown() {}

fn run_test<T>(test: T) -> ()
where T: FnOnce() -> () + panic::UnwindSafe
{
    setup();
    let result = panic::catch_unwind(|| {
        test()
    });
    teardown();
    assert!(result.is_ok())
}

struct SkateError {
    exit_code: i32,
    message: String,
}

impl Debug for SkateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkateError")
            .field("exit_code", &self.exit_code)
            .field("message", &self.message)
            .finish()
    }
}

fn skate(command: &str, args: &[&str]) -> Result<String, SkateError> {
    let output = process::Command::new(command)
        .args(args)
        .output().map_err(|e| SkateError{exit_code: -1, message: e.to_string()})?;
    if !output.status.success() {
        return Err(SkateError{exit_code: output.status.code().unwrap_or_default(), message: String::from_utf8_lossy(&output.stderr).to_string()});
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}

fn ips() -> (String, String) {
    let output = process::Command::new("../hack/clusterplz")
        .args(&["ips"])
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines();
    (lines[0], lines[1])
}

#[test]
#[ignore]
fn test_node_creation() {

    run_test(|| {

        let result = skate("create", &["node"]);

    })

}