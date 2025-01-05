use std::{env, panic, process, time};
use tokio::process::{Command};
use tokio::time::sleep;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::io::{stderr, stdout};
use std::time::Duration;
use anyhow::anyhow;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::error;
use serde_json::Value;

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

pub async fn retry<F, Fu>(attempts: u8, delay: u64, f: F) -> Result<(), ()>
where
    F: Fn() -> Fu,
    Fu: Future<Output=Result<(), ()>>,
{
    for n in 0..attempts {
        if n >= 1 {
            println!("retried {} times", n);
        }

        if let Ok(()) = f().await {
            return Ok(());
        }

        sleep(Duration::from_secs(delay)).await;
    }

    println!("error after {} attempts", attempts);

    Err(())
}

async fn skate(command: &str, args: &[&str]) -> Result<(String, String), SkateError> {
    let output = Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .output().await.map_err(|e| SkateError { exit_code: -1, message: e.to_string() })?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(SkateError { exit_code: output.status.code().unwrap_or_default(), message: stderr });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok((stdout, stderr))
}

async fn skate_stdout(command: &str, args: &[&str]) -> Result<(), SkateError> {
    let mut child = Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .stdout(stdout())
        .stderr(stderr())
        .spawn().map_err(|e| SkateError { exit_code: -1, message: e.to_string() })?;


    let status = child.wait().await.map_err(|e| SkateError { exit_code: -1, message: e.to_string() })?;
    if !status.success() {
        return Err(SkateError { exit_code: status.code().unwrap_or_default(), message: "".to_string() });
    }

    Ok(())
}


#[tokio::test]
async fn e2e_test() {
    if env::var("SKATE_E2E").is_err() {
        return;
    }

    skate_stdout("config", &["use-context", "e2e-test"]).await.expect("failed to set context");

    test_cluster_creation().await.expect("failed to create cluster");
    test_deployment().await.expect("failed to test deployment");
    test_service().await.expect("failed to test service");
}

async fn test_cluster_creation() -> Result<(), anyhow::Error> {

    // let user = env::var("USER")?;
    //
    // skate_stdout("delete", &["cluster", "integration-test", "--yes"]);
    // skate_stdout("create", &["cluster", "integration-test"])?;
    // skate_stdout("config", &["use-context", "integration-test"])?;
    // skate_stdout("create", &["node", "--name", "node-1", "--host", &addrs.0, "--subnet-cidr", "20.1.0.0/16", "--key", "/tmp/skate-e2e-key", "--user", &user])?;
    // skate_stdout("create", &["node", "--name", "node-2", "--host", &addrs.1, "--subnet-cidr", "20.2.0.0/16", "--key", "/tmp/skate-e2e-key", "--user", &user])?;
    let (stdout, _stderr) = skate("refresh", &["--json"]).await?;

    let state: Value = serde_json::from_str(&stdout)?;

    assert_eq!(state["nodes"].as_array().unwrap().len(), 2);
    let node1 = state["nodes"][0].clone();
    let node2 = state["nodes"][1].clone();

    assert_eq!(node1["node_name"], "node-1");
    assert_eq!(node1["status"], "Healthy");
    assert_eq!(node2["node_name"], "node-2");
    assert_eq!(node2["status"], "Healthy");

    Ok(())
}
async fn test_deployment() -> Result<(), anyhow::Error> {
    let root = env::var("CARGO_MANIFEST_DIR")?;

    skate_stdout("apply", &["-f", &format!("{root}/tests/manifests/test-deployment.yaml")]).await?;

    let output = skate("get", &["pods", "-n", "test-deployment"]).await?;

    println!("{}", output.0);

    let stdout = output.0;

    let lines = stdout.lines().skip(1);

    assert_eq!(lines.clone().count(), 3);

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 6 {
            assert_eq!(parts[0], "test-deployment");
            assert_eq!(true, parts[1].starts_with("dpl-nginx-"));
            assert_eq!(parts[2], "2/2");
            assert_eq!(parts[3], "Running");
            assert_eq!(parts[4], "0");
        }
    }

    let fut: FuturesUnordered<_> = ["node-1", "node-2"].iter().map(|node| async {
        let result = retry(10, 1, || async {
            match skate("node-shell", &[node, "--", "dig", "+short", "nginx.test-deployment.pod.cluster.skate"]).await {
                Ok((stdout, _)) => {
                    if stdout.trim().lines().count() != 3 {
                        return Err(());
                    }
                    Ok(())
                }
                Err(err) => {
                    error!("{:?}", err );
                    Err(())
                }
            }
        }).await;
        assert!(result.is_ok());
        Result::<(), anyhow::Error>::Ok(())
    }).collect();

    let _: Vec<_> = fut.collect().await;

    // TODO - check healthchecks work
    //      - dns entries exist
    //      - addresses are reachable from each node


    Ok(())
}

async fn test_service() -> Result<(), anyhow::Error> {
    let root = env::var("CARGO_MANIFEST_DIR")?;

    skate_stdout("apply", &["-f", &format!("{root}/tests/manifests/test-service.yaml")]).await?;

    let output = skate("get", &["service", "-n", "test-deployment"]).await?;

    println!("{}", output.0);

    let stdout = output.0;

    let lines = stdout.lines().skip(1);

    assert_eq!(lines.clone().count(), 1);

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 6 {
            assert_eq!(parts[0], "test-deployment");
            assert_eq!(true, parts[1].starts_with("nginx"));
            assert_eq!(parts[4], "80");
        }
    }


    let fut: FuturesUnordered<_> = ["node-1", "node-2"].iter().map(|node| async {
        let (stdout, _) = skate("node-shell", &[node, "--", "pgrep", "-x", "keepalived"]).await?;
        // keepalived 2 has 2 processes
        assert_eq!(stdout.trim().lines().count(), 2);

        let result = retry(10, 1, || async {
            match skate("node-shell", &[node, "--", "dig", "+short", "nginx.test-deployment.svc.cluster.skate"]).await {
                Ok((stdout, _)) => {
                    if stdout.trim().lines().count() != 1 {
                        return Err(());
                    }
                    Ok(())
                }
                Err(err) => {
                    error!("{}", err );
                    Err(())
                }
            }
        }).await;
        assert!(result.is_ok());
        Result::<(), anyhow::Error>::Ok(())
    }).collect();
    let _: Vec<_> = fut.collect().await;

    // TODO
    //      - keepalived realservers exist
    //      - service is reachable
    Ok(())
}
