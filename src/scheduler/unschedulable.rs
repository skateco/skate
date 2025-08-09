use crate::scheduler::plugins::{Filter, Plugin};
use crate::state::state::{NodeState, NodeStatus};
use k8s_openapi::api::core::v1::Pod;

pub(crate) struct UnschedulableFilter {}

impl Plugin for UnschedulableFilter {
    fn name(&self) -> &'static str {
        "UnschedulableFilter"
    }
}

impl Filter for UnschedulableFilter {
    fn filter(&self, _: &Pod, node: &NodeState) -> Result<(), String> {
        if node.status != NodeStatus::Healthy {
            return Err("Node is unschedulable".to_string());
        }
        Ok(())
    }
}
