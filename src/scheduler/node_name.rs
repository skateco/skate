use crate::scheduler::plugins::Filter;
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::Pod;

pub(crate) struct NodeNameFilter {}

impl Filter for NodeNameFilter {
    fn filter(&self, pod: &Pod, node: &NodeState) -> Result<(), String> {
        if !fits(pod, node) {
            return Err("unschedulable and unresolvable".to_string());
        }
        Ok(())
    }
}

pub fn fits(pod: &Pod, node: &NodeState) -> bool {
    let def = "".to_string();
    let node_name = pod
        .spec
        .as_ref()
        .and_then(|spec| spec.node_name.as_ref())
        .unwrap_or(&def);

    node_name.len() == 0 || node_name == &node.node_name
}
