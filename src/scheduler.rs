use std::error::Error;
use async_trait::async_trait;
use crate::config::Node;
use crate::skate::SupportedResources;
use crate::ssh::HostInfoResponse;

#[derive(Debug)]
pub struct CandidateNode {
    pub info: HostInfoResponse,
    pub node: Node
}

#[derive(Debug)]
pub struct ScheduleResult {
    object: SupportedResources,
    node: CandidateNode,
}

#[async_trait]
pub trait Scheduler {

    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>>;
}

pub struct DefaultScheduler {}

#[async_trait]
impl Scheduler for DefaultScheduler {
    async fn schedule(&self, nodes: Vec<CandidateNode>, objects: Vec<SupportedResources>) -> Result<ScheduleResult, Box<dyn Error>> {
        todo!()
    }

}
