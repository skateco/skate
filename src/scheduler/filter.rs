use crate::scheduler::score::Filter;
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::{Node as K8sNode, Pod};

pub struct DefaultFilter {}

impl Filter for DefaultFilter {
    fn filter(&self, pod: &Pod, n: &NodeState) -> Result<(), String> {
        let k8s_node: K8sNode = n.into();
        let node_labels = k8s_node.metadata.labels.unwrap_or_default();
        // only schedulable nodes
        let is_schedulable = k8s_node
            .spec
            .and_then(|s| s.unschedulable.map(|u| !u))
            .unwrap_or(false);

        if !is_schedulable {
            return Err("node is unschedulable".to_string());
        }

        let node_selector = pod
            .spec
            .as_ref()
            .and_then(|spec| spec.node_selector.as_ref());
        if node_selector.is_none() {
            return Ok(()); // no node selector, so all nodes match
        }

        // only nodes that match the nodeselectors
        for (k, v) in node_selector.unwrap().iter() {
            if node_labels.get(k).unwrap_or(&"".to_string()) != v {
                return Err(format!("node selector {}:{} did not match", k, v));
            }
        }
        Ok(())
    }
}
