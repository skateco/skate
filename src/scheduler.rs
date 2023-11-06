use std::error::Error;
use async_trait::async_trait;
use crate::config::Node;
use crate::scheduler::Status::{Error as ScheduleError, Scheduled};
use crate::skate::SupportedResources;
use crate::ssh::{HostInfoResponse, SshClients};
use crate::state::state::State;

#[derive(Debug)]
pub struct CandidateNode {
    pub info: HostInfoResponse,
    pub node: Node,
}

#[derive(Debug)]
pub enum Status {
    Scheduled(String),
    Error(String),
}

#[derive(Debug)]
pub struct ScheduleResult {
    pub object: SupportedResources,
    pub node_name: String,
    pub status: Status,
}

#[async_trait]
pub trait Scheduler {
    async fn schedule(&self, conns: SshClients, prev_state: &mut State, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>>;
}

pub struct DefaultScheduler {}

#[async_trait]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, conns: SshClients, prev_state: &mut State, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>> {
        let node_name = &(nodes).first().ok_or("no nodes")?.node.name;

        let client = conns.find(node_name).ok_or("failed to find connection for node")?;

        let mut results: Vec<ScheduleResult> = vec![];
        for object in objects {
            match object {
                SupportedResources::Pod(_) | SupportedResources::Deployment(_) => {
                    let serialized = serde_yaml::to_string(&object)?;
                    println!("scheduling {} on node {}", object, node_name);
                    let result = client.apply_resource(&serialized).await;
                    results.push(ScheduleResult {
                        object,
                        node_name: node_name.clone(),
                        status: match result {
                            Ok((stdout, stderr)) => {
                                let mut builder = String::new();
                                builder.push_str(&stdout);
                                if stderr.len() > 0 {
                                    builder.push_str(&format!(" ( stderr: {} )", stderr))
                                }
                                Scheduled(format!("{}", builder.replace("\n", "\\n")))
                            }
                            Err(err) => ScheduleError(err.to_string())
                        },
                    });
                }
            }
        }
        Ok(results)
    }
}
