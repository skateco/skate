use crate::scheduler::score::PreFilter;
use crate::state::state::NodeState;
use k8s_openapi::api::core::v1::Pod;
use std::error::Error;

pub struct DefaultPreFilter {}

impl PreFilter for DefaultPreFilter {
    fn pre_filter(&self, _pod: &Pod, _nodes: &[NodeState]) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
