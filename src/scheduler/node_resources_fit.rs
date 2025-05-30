use crate::scheduler::plugins::{Filter, Score};
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::Pod;
use std::error::Error;

pub(crate) struct NodeResourcesFit {}

impl Filter for NodeResourcesFit {
    fn filter(&self, pod: &Pod, node: &NodeState) -> Result<(), String> {
        todo!()
    }
}
