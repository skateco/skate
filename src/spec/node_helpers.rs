use crate::spec::pod_helpers;
use crate::spec::pod_helpers::get_requests;
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::{Pod, PodSpec};

/// get the sum of the pod requests on the node
/// pass default (cpu, mem) to use if the pod has none
pub fn get_node_requests(
    n: &NodeState,
    default: Option<(u64, u64)>,
) -> Result<(u64, u64), pod_helpers::Error> {
    let mut total_cpu_millis = 0;
    let mut total_mem_bytes = 0;

    for p in &n.filter_pods(&|_| true) {
        let k8s_pod: Pod = p.into();
        let spec = match k8s_pod.spec.as_ref() {
            None => continue,
            Some(spec) => spec,
        };

        let (pod_cpu, pod_mem) = requests_or_default(spec, default)?;
        total_cpu_millis += pod_cpu;
        total_mem_bytes += pod_mem;
    }
    Ok((total_cpu_millis, total_mem_bytes))
}

fn requests_or_default(
    p: &PodSpec,
    default: Option<(u64, u64)>,
) -> Result<(u64, u64), pod_helpers::Error> {
    let r = get_requests(p)?;
    let cpu = r
        .cpu_millis
        .or(default.and_then(|d| Some(d.0)))
        .unwrap_or(0);
    let memory = r
        .memory_bytes
        .or(default.and_then(|d| Some(d.1)))
        .unwrap_or(0);
    Ok((cpu, memory))
}

/// get the allocatable cpu and mem for a node
/// Assumes that all cpu and mem are allocatable :scream:
///
/// TODO - introduce concept of what is allocatable and have a reservation for the os/base services
pub fn get_node_alloc(n: &NodeState) -> (u64, u64) {
    let si = match n.system_info() {
        None => return (0, 0),
        Some(si) => si,
    };

    let total_cpu_millis = si.cpu_total_millis();
    let total_mem_bytes = si.total_memory_mib * 1024 * 1024;

    (total_cpu_millis as u64, total_mem_bytes)
}

#[cfg(test)]
mod tests {
    use crate::spec::node_helpers::get_node_alloc;
    use crate::test_helpers::objects::node_state;

    #[test]
    fn should_get_node_alloc() {
        let mut node = node_state("node1");
        let mut si = node
            .host_info
            .as_mut()
            .unwrap()
            .system_info
            .as_mut()
            .unwrap()
            .clone();
        si.total_memory_mib = 1000;
        si.cpu_usage = 50.0; // shouldn't affect allocatable
        si.num_cpus = 4;

        let node_alloc = get_node_alloc(&node);
        assert_eq!(node_alloc.0, 1000); // 4 CPUs * 1000 millis each
        assert_eq!(node_alloc.1, 1000 * 1024 * 1024); // 1000 MiB in bytes
    }
}
