use anyhow::anyhow;
use colored::Colorize;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use serde_json::Value;
use std::env;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::io::{stderr, stdout};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

#[derive(Debug, Clone)]
struct SkateError {
    exit_code: i32,
    message: String,
}

impl Error for SkateError {}

impl Display for SkateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit code: {}, message: {}",
            self.exit_code, self.message
        )
    }
}

pub async fn retry<F, Fu, R>(attempts: u8, delay: u64, f: F) -> Result<R, anyhow::Error>
where
    F: Fn() -> Fu,
    Fu: Future<Output = Result<R, anyhow::Error>>,
{
    for n in 0..attempts {
        if n >= 1 {
            println!("retried {} times", n);
        }

        if let Ok(res) = f().await {
            return Ok(res);
        }

        sleep(Duration::from_secs(delay)).await;
    }

    Err(anyhow!("error after {} attempts", attempts))
}

pub async fn retry_all_nodes<F, Fu, R>(
    attempts: u8,
    delay: u64,
    f: F,
) -> Vec<Result<R, anyhow::Error>>
where
    F: Fn(String) -> Fu,
    Fu: Future<Output = Result<R, anyhow::Error>>,
{
    let fut: FuturesUnordered<_> = ["node-1", "node-2"]
        .into_iter()
        .map(|node| async {
            for n in 0..attempts {
                if n >= 1 {
                    println!("retried {} times", n);
                }

                let result = f(node.to_string()).await;
                if let Ok(res) = result {
                    return Ok(res);
                } else if n == attempts - 1 {
                    return Err(anyhow!(
                        "{} => error after {} attempts: {:?}",
                        // don't let clippy fool you, tests will not build if you pass the item directly
                        node.to_owned(),
                        attempts,
                        result.err()
                    ));
                }

                sleep(Duration::from_secs(delay)).await;
            }
            Err(anyhow!(
                "{} => error after {} attempts: ? shouldn't get here ?",
                // don't let clippy fool you, tests will not build if you pass the item directly
                node.to_owned(),
                attempts
            ))
        })
        .collect();

    let result: Vec<_> = fut.collect().await;
    result
}

async fn skate(command: &str, args: &[&str]) -> Result<(String, String), SkateError> {
    println!(
        "running command: {}",
        [&["skate", command], args].concat().join(" ").green()
    );
    let output = Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .output()
        .await
        .map_err(|e| SkateError {
            exit_code: -1,
            message: e.to_string(),
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(SkateError {
            exit_code: output.status.code().unwrap_or_default(),
            message: stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok((stdout, stderr))
}

async fn skate_stdout(command: &str, args: &[&str]) -> Result<(), SkateError> {
    let mut child = Command::new("./target/debug/skate")
        .args([&[command], args].concat())
        .stdout(stdout())
        .stderr(stderr())
        .spawn()
        .map_err(|e| SkateError {
            exit_code: -1,
            message: e.to_string(),
        })?;

    let status = child.wait().await.map_err(|e| SkateError {
        exit_code: -1,
        message: e.to_string(),
    })?;
    if !status.success() {
        return Err(SkateError {
            exit_code: status.code().unwrap_or_default(),
            message: "".to_string(),
        });
    }

    Ok(())
}

#[tokio::test]
async fn e2e_test() {
    if env::var("SKATE_E2E").is_err() {
        return;
    }

    skate_stdout("config", &["use-context", "sind"])
        .await
        .expect("failed to set context");

    test_cluster_creation()
        .await
        .expect("failed to create cluster");
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

    assert_eq!(node1["node_name"], "sind-node-1");
    assert_eq!(node1["status"], "Healthy");
    assert_eq!(node2["node_name"], "sind-node-2");
    assert_eq!(node2["status"], "Healthy");

    Ok(())
}

async fn curl_for_content(node: String, url: String, content: String) -> Result<(), anyhow::Error> {
    let (stdout, _stderr) = skate(
        "node-shell",
        &[&node, "--", "curl", "--fail", "--silent", &url],
    )
    .await?;
    if !stdout.contains(&content) {
        return Err(anyhow!("{}: expected {}, got {}", node, content, stdout));
    }
    Ok(())
}
async fn test_deployment() -> Result<(), anyhow::Error> {
    let root = env::var("CARGO_MANIFEST_DIR")?;

    skate_stdout(
        "apply",
        &[
            "-f",
            &format!("{root}/tests/manifests/test-deployment.yaml"),
        ],
    )
    .await?;

    let output = skate("get", &["pods", "-n", "test-deployment"]).await?;

    println!("{}", output.0);

    let stdout = output.0;

    let lines = stdout.lines().skip(1);

    assert_eq!(lines.clone().count(), 3);

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 6 {
            assert_eq!(parts[0], "test-deployment");
            assert!(parts[1].starts_with("dpl-nginx-"));
            assert_eq!(parts[2], "2/2");
            assert_eq!(parts[3], "Running");
            assert_eq!(parts[4], "0");
        }
    }

    let results = retry_all_nodes(10, 1, |node: String| async move {
        let (stdout, _) = skate(
            "node-shell",
            &[
                &node,
                "--",
                "dig",
                "+short",
                "nginx.test-deployment.pod.cluster.skate",
            ],
        )
        .await?;

        let ips = stdout.trim().lines().collect::<Vec<_>>();

        if ips.len() != 3 {
            return Err(anyhow!(
                "expected 3 dns entries, got {}: {:?}",
                ips.len(),
                ips,
            ));
        }

        let futures: FuturesUnordered<_> = ips
            .iter()
            .map(|ip| {
                curl_for_content(
                    node.to_string(),
                    ip.to_string(),
                    "Welcome to nginx!".to_string(),
                )
            })
            .collect();

        let results: Vec<_> = futures.collect().await;

        assert!(results.iter().all(|r| r.is_ok()), "{:?}", results);

        // TODO - check healthchecks work

        Ok(())
    })
    .await;

    assert!(results.iter().all(|r| r.is_ok()), "{:?}", results);

    Ok(())
}

async fn test_service() -> Result<(), anyhow::Error> {
    let root = env::var("CARGO_MANIFEST_DIR")?;

    skate_stdout(
        "apply",
        &["-f", &format!("{root}/tests/manifests/test-service.yaml")],
    )
    .await?;

    let output = skate("get", &["service", "-n", "test-deployment"]).await?;

    println!("{}", output.0);

    let stdout = output.0;

    let lines = stdout.lines().skip(1);

    assert_eq!(lines.clone().count(), 1);

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 6 {
            assert_eq!(parts[0], "test-deployment");
            assert!(parts[1].starts_with("nginx"));
            assert_eq!(parts[4], "80");
        }
    }

    let results = retry_all_nodes(10, 1, |node: String| async move {
        let (stdout, _) = skate("node-shell", &[&node, "--", "pgrep", "-x", "keepalived"]).await?;

        // keepalived 2 has 2 processes
        let procs = stdout.trim().lines().count();
        if procs != 2 {
            return Err(anyhow!("expected 2 keepalived processes, got {}", procs));
        }
        Ok(())
    })
    .await;

    assert!(results.iter().all(|r| r.is_ok()));
    let host = "nginx.test-deployment.svc.cluster.skate";

    let results = retry_all_nodes(10, 1, |node: String| async move {
        let (stdout, _) = skate("node-shell", &[&node, "--", "dig", "+short", host]).await?;

        let service_ips = stdout.trim().lines().collect::<Vec<_>>();

        if service_ips.len() != 1 {
            return Err(anyhow!(
                "expected 1 dns entry, got {}: {:?}",
                service_ips.len(),
                service_ips,
            ));
        }

        for _ in 0..5 {
            curl_for_content(
                node.to_string(),
                host.to_string(),
                "Welcome to nginx!".to_string(),
            )
            .await?;
        }
        let service_ip = service_ips[0];

        ensure_lvs_realservers(service_ip.to_string(), 80, 3).await?;

        Ok(())
    })
    .await;

    assert!(results.iter().all(|r| r.is_ok()), "{:?}", results);

    Ok(())
}

async fn ensure_lvs_realservers(
    vs_host: String,
    vs_port: u16,
    assert_realservers: usize,
) -> Result<(), anyhow::Error> {
    let (stdout, _stderr) = skate(
        "node-shell",
        &[
            "node-1",
            "--",
            "sudo",
            "ipvsadm",
            "-L",
            "-n",
            "-t",
            &format!("{}:{}", vs_host, vs_port),
        ],
    )
    .await
    .map_err(|e| anyhow!(e).context("ipvsadm command failed"))?;

    let lines = stdout.trim().lines().collect::<Vec<_>>();

    let realservers = lines.iter().skip(3);

    let realservers = realservers.collect::<Vec<_>>();

    if realservers.len() != assert_realservers {
        return Err(anyhow!(
            "expected {} realservers, got {}: {:?}",
            assert_realservers,
            realservers.len(),
            realservers,
        ));
    }

    Ok(())
}
