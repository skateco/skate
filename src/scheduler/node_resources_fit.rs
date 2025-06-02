use crate::scheduler::plugins::{Filter, Plugin, PreFilter, Score};
use crate::scheduler::pod_scheduler::{DEFAULT_MEMORY_REQUEST, DEFAULT_MILLI_CPU_REQUEST};
use crate::spec::pod_helpers;
use crate::spec::pod_helpers::{get_requests, ResourceRequests};
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::{Pod, PodSpec};
use std::error::Error;

pub(crate) struct NodeResourcesFit {}

impl NodeResourcesFit {
    fn requests_or_default(p: &PodSpec) -> Result<(u64, u64), pod_helpers::Error> {
        let r = get_requests(p)?;
        let cpu = r.cpu_millis.unwrap_or(DEFAULT_MILLI_CPU_REQUEST); // default to 100m if not specified
        let memory = r.memory_bytes.unwrap_or(DEFAULT_MEMORY_REQUEST); // default to 200Mi if not specified
        Ok((cpu, memory))
    }
}

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
        let (cpu_request, memory_request) =
            Self::requests_or_default(pod.spec.as_ref().ok_or("no pod spec")?)
                .map_err(|e| e.to_string())?;

        let si = node.system_info().ok_or("no node system info")?;

        let mut total_cpu = 0;
        let mut total_mem = 0;

        for p in &node.filter_pods(&|_| true) {
            let k8s_pod: Pod = p.into();
            let spec = k8s_pod.spec.as_ref().ok_or("no pod spec")?;
            let (pod_cpu, pod_mem) = Self::requests_or_default(spec).map_err(|e| e.to_string())?;
            total_cpu += pod_cpu;
            total_mem += pod_mem;
        }

        let available_cpu_millis = (si.num_cpus as u64) * 1000 - total_cpu; // convert cores to milliCPU
        let available_mem_bytes = (si.total_memory_mib) * 1024 * 1024 - total_mem; // convert MiB to bytes

        if available_cpu_millis < cpu_request {
            return Err(format!(
                "Node {} has insufficient CPU: requested {}m, available {}m",
                node.node_name, cpu_request, available_cpu_millis
            ));
        }
        if available_mem_bytes < memory_request {
            return Err(format!(
                "Node {} has insufficient memory: requested {} bytes, available {} bytes",
                node.node_name, memory_request, available_mem_bytes
            ));
        }

        Ok(())
    }
}

impl Score for NodeResourcesFit {
    // see https://github.com/kubernetes/kubernetes/blob/master/pkg/scheduler/framework/plugins/noderesources/resource_allocation.go#L48
    fn score(&self, pod: &Pod, node: &NodeState) -> Result<u32, Box<dyn Error>> {
        Ok(0)
    }
}
