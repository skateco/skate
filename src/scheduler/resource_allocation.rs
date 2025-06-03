use crate::scheduler::node_resources_fit::requests_or_default;
use crate::scheduler::plugins::{Plugin, Score, ScoreError, MAX_NODE_SCORE};
use crate::scheduler::pod_scheduler::{DEFAULT_MEMORY_REQUEST, DEFAULT_MILLI_CPU_REQUEST};
use crate::spec::node_helpers::{get_node_alloc, get_node_requests};
use crate::spec::pod_helpers::ResourceRequests;
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::Pod;
use std::error::Error;

// strum error enum

struct ResourceAllocationScorer {
    name: String,
    resources: ResourceRequests,
}

impl Plugin for ResourceAllocationScorer {
    fn name(&self) -> &str {
        self.name.as_str()
    }
}

pub struct NodeResourceAllocVsReq {
    pub allocatable_cpu_millis: u64,
    pub allocatable_mem_bytes: u64,
    pub req_cpu_millis: u64,
    pub req_mem_bytes: u64,
}
impl ResourceAllocationScorer {
    pub fn calc_node_resource_alloc_req(
        &self,
        node_state: &NodeState,
        (pod_cpu, pod_mem): (u64, u64),
    ) -> NodeResourceAllocVsReq {
        let (req_cpu_millis, req_mem_bytes) = get_node_requests(
            node_state,
            Some((DEFAULT_MILLI_CPU_REQUEST, DEFAULT_MEMORY_REQUEST)),
        )
        .unwrap_or((0, 0));

        let (alloc_cpu_millis, alloc_mem_bytes) = get_node_alloc(node_state);

        NodeResourceAllocVsReq {
            allocatable_cpu_millis: alloc_cpu_millis,
            allocatable_mem_bytes: alloc_mem_bytes,
            req_cpu_millis: req_cpu_millis + pod_cpu,
            req_mem_bytes: req_mem_bytes + pod_mem,
        }
    }

    fn least_requested_score(requested: u64, capacity: u64) -> u64 {
        if capacity == 0 {
            return 0; // Avoid division by zero
        }
        if requested > capacity {
            return 0;
        }
        ((capacity - requested) * MAX_NODE_SCORE) / capacity
    }
}

impl Score for ResourceAllocationScorer {
    /// Score does a Least Allocated strategy
    /// TODO - add Most Allocated and Requested to Capacity strategies
    /// TODO - allow weights
    fn score(&self, pod: &Pod, node: &NodeState) -> Result<u64, ScoreError> {
        let spec = if let Some(spec) = pod.spec.as_ref() {
            spec
        } else {
            return Err(ScoreError::PodSpecEmpty);
        };

        let requests = requests_or_default(spec)?;
        let alloc_v_req = self.calc_node_resource_alloc_req(node, requests);

        // cpu
        let cpu_weight = 1;
        let mem_weight = 1;

        let cpu_score = cpu_weight
            * Self::least_requested_score(
                alloc_v_req.req_cpu_millis,
                alloc_v_req.allocatable_cpu_millis,
            );
        let mem_score = mem_weight
            * Self::least_requested_score(
                alloc_v_req.req_mem_bytes,
                alloc_v_req.allocatable_mem_bytes,
            );

        let sum_weight = cpu_weight + mem_weight;

        if sum_weight == 0 {
            return Ok(0);
        }

        let node_score = (cpu_score + mem_score) / sum_weight;

        Ok(node_score / sum_weight)
    }
}
