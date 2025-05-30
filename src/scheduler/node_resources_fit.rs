use crate::scheduler::plugins::{Filter, Plugin, PreFilter, Score};
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::Pod;
use std::error::Error;

pub(crate) struct NodeResourcesFit {}

impl Plugin for NodeResourcesFit {
    fn name(&self) -> &'static str {
        "NodeResourcesFit"
    }
}

impl PreFilter for NodeResourcesFit {
    fn pre_filter(&self, pod: &Pod, nodes: &[NodeState]) -> Result<(), Box<dyn Error>> {
        // calculate the total resources required by the pod and cache them
        Ok(())
    }
}

impl Filter for NodeResourcesFit {
    // Checks if a node has sufficient resources, such as cpu, memory, gpu, opaque int resources etc to run a pod.
    // It returns a list of insufficient resources, if empty, then the node has all the resources requested by the pod.
    fn filter(&self, pod: &Pod, node: &NodeState) -> Result<(), String> {
        Ok(())
    }
}

impl Score for NodeResourcesFit {
    // see https://github.com/kubernetes/kubernetes/blob/master/pkg/scheduler/framework/plugins/noderesources/resource_allocation.go#L48
    fn score(&self, pod: &Pod, node: &NodeState) -> Result<u32, Box<dyn Error>> {
        Ok(0)
    }
}
