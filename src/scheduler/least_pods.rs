use crate::scheduler::score::Score;
use crate::state::state::NodeState;
use std::collections::HashMap;
use std::error::Error;

pub(crate) struct LeastPods {}

impl Score for LeastPods {
    fn score(
        &self,
        pod: &k8s_openapi::api::core::v1::Pod,
        node: &NodeState,
    ) -> Result<u32, Box<dyn std::error::Error>> {
        if let Some(si) = node.system_info() {
            Ok(si
                .pods
                .as_ref()
                .and_then(|p| Some(p.len()))
                .unwrap_or_default() as u32)
        } else {
            Ok(0)
        }
    }
}
