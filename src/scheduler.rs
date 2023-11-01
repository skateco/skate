use std::error::Error;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Pod;
use crate::config::Node;
use crate::skate::{Distribution, Os, Platform, SupportedResources};
use crate::ssh::HostInfoResponse;

#[derive(Debug)]
pub struct CandidateNode {
    pub info: HostInfoResponse,
    pub node: Node
}

#[derive(Debug)]
pub struct ScheduleResult {
    pub object: SupportedResources,
    pub node: CandidateNode,
}

#[async_trait]
pub trait Scheduler {

    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>>;
}

pub struct DefaultScheduler {}

#[async_trait]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>> {
        Ok(ScheduleResult{ object: SupportedResources::Pod(Pod{
            metadata: Default::default(),
            spec: None,
            status: None,
        }), node: CandidateNode { info: HostInfoResponse {
            node_name: "".to_string(),
            hostname: "".to_string(),
            platform: Platform {
                arch: "".to_string(),
                os: Os::Unknown,
                distribution: Distribution::Unknown,
            },
            skatelet_version: None,
        }, node: Node {
            name: "".to_string(),
            host: "".to_string(),
            port: None,
            user: None,
            key: None,
        } } })
    }

}
