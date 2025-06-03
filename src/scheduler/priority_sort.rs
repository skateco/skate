use crate::scheduler::plugins::{Plugin, QueueSort};
use k8s_openapi::api::core::v1::Pod;

pub(crate) struct PrioritySort {}

pub fn pod_priority(pod: &Pod) -> i32 {
    pod.spec
        .as_ref()
        .and_then(|spec| spec.priority)
        .unwrap_or(0)
}
impl Plugin for PrioritySort {
    fn name(&self) -> &'static str {
        "PrioritySort"
    }
}
impl QueueSort for PrioritySort {
    fn less(&self, pod1: &Pod, pod2: &Pod) -> bool {
        let p1 = pod_priority(pod1);
        let p2 = pod_priority(pod2);
        // k8s orders earlier pods first, but we don't have that info
        p1 > p2
    }
}
