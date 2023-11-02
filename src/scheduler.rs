use std::error::Error;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Pod;
use crate::config::Node;
use crate::skate::{Distribution, Os, Platform, SupportedResources};
use crate::ssh::HostInfoResponse;

#[derive(Debug)]
pub struct CandidateNode {
    pub info: HostInfoResponse,
    pub node: Node,
}

#[derive(Debug)]
pub enum Status {
    Scheduled,
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
    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>>;
}

pub struct DefaultScheduler {}

#[async_trait]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<Vec<ScheduleResult>, Box<dyn Error>> {
        Ok(vec![ScheduleResult {
            object: SupportedResources::Pod(Pod {
                metadata: Default::default(),
                spec: None,
                status: None,
            }),
            node_name: "".to_string(),
            status: Status::Error("failed".to_string()),
        }])
    }
}
