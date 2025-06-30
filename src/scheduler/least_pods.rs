use crate::scheduler::plugins::{inverted_normalize_scores, Plugin, Score, ScoreError};
use crate::skatelet::system::podman::PodmanPodStatus;
use crate::state::state::NodeState;
use std::collections::BTreeMap;

/// LeastPods is a scoring plugin that scores nodes based on the number of pods they are currently running.
pub(crate) struct LeastPods {}

impl Plugin for LeastPods {
    fn name(&self) -> &'static str {
        "LeastPods"
    }
}

impl Score for LeastPods {
    fn score(
        &self,
        pod: &k8s_openapi::api::core::v1::Pod,
        node: &NodeState,
    ) -> Result<u64, ScoreError> {
        if let Some(si) = node.system_info() {
            Ok(si
                .pods
                .as_ref()
                .and_then(|p| {
                    Some(
                        p.iter()
                            .filter(|p| {
                                !matches!(p.status, PodmanPodStatus::Dead | PodmanPodStatus::Exited)
                            })
                            .count(),
                    )
                })
                .unwrap_or_default() as u64)
        } else {
            Ok(0)
        }
    }

    /// Since we want the node with the least number of pods to have the highest score,
    fn normalize_scores(&self, scores: &mut BTreeMap<String, u64>) -> Result<(), ScoreError> {
        inverted_normalize_scores(scores)
    }
}

mod tests {

    #[test]
    fn test_least_pods() {
        use super::*;
        use crate::test_helpers::objects::node_state;
        use k8s_openapi::api::core::v1::Pod;

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: None,
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                phase: Some("Running".to_string()),
                ..Default::default()
            }),
        };

        let node1 = node_state("test-node-1").with_pod(&pod);

        let node2 = node_state("test-node-2")
            .with_pod(&pod)
            .with_pod(&pod)
            .with_pod(&pod);

        let node3 = node_state("test-node-3")
            .with_pod(&pod)
            .with_pod(&pod)
            .with_pod(&pod)
            .with_pod(&pod)
            .with_pod(&pod);

        dbg!(&node1);

        let mut scores = BTreeMap::new();
        let least_pods = LeastPods {};
        scores.insert(
            "test-node-1".to_string(),
            least_pods.score(&pod, &node1).unwrap(),
        );
        scores.insert(
            "test-node-2".to_string(),
            least_pods.score(&pod, &node2).unwrap(),
        );
        scores.insert(
            "test-node-3".to_string(),
            least_pods.score(&pod, &node3).unwrap(),
        );
        dbg!(&scores);

        least_pods.normalize_scores(&mut scores).unwrap();
        dbg!(&scores);

        assert_eq!(scores.len(), 3);

        assert_eq!(scores.get("test-node-3").unwrap(), &0);
        assert_eq!(scores.get("test-node-2").unwrap(), &50);
        assert_eq!(scores.get("test-node-1").unwrap(), &100);
    }

    #[test]
    fn test_least_pods_none_running() {
        let node = node_state("test-node-1");

        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: None,
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                phase: Some("Failed".to_string()),
                ..Default::default()
            }),
        };

        let node = node.with_pod(&pod);

        let least_pods = LeastPods {};
        let result = least_pods.score(&pod, &node).unwrap();
        assert_eq!(result, 0, "Expected score to be 0 when no pods are running");
    }
}
