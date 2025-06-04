use crate::spec::pod_helpers;
use crate::state::state::NodeState;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use std::collections::BTreeMap;
use std::error::Error;
use std::ops::DerefMut;

///
/// func getDefaultPlugins() *v1.Plugins {
// 	plugins := &v1.Plugins{
// 		MultiPoint: v1.PluginSet{
// 			Enabled: []v1.Plugin{
// 				{Name: names.SchedulingGates}, No
// 				{Name: names.PrioritySort}, yes
// 				{Name: names.NodeUnschedulable}, yes
// 				{Name: names.NodeName}, yes
// 				{Name: names.TaintToleration, Weight: ptr.To[int32](3)}, TODO
// 				{Name: names.NodeAffinity, Weight: ptr.To[int32](2)}, TODO
// 				{Name: names.NodePorts}, TODO
// 				{Name: names.NodeResourcesFit, Weight: ptr.To[int32](1)}, TODO
// 				{Name: names.VolumeRestrictions},
// 				{Name: names.NodeVolumeLimits},
// 				{Name: names.VolumeBinding},
// 				{Name: names.VolumeZone},
// 				{Name: names.PodTopologySpread, Weight: ptr.To[int32](2)},
// 				{Name: names.InterPodAffinity, Weight: ptr.To[int32](2)},
// 				{Name: names.DefaultPreemption},
// 				{Name: names.NodeResourcesBalancedAllocation, Weight: ptr.To[int32](1)},
// 				{Name: names.ImageLocality, Weight: ptr.To[int32](1)},
// 				{Name: names.DefaultBinder},
// 			},
// 		},
// 	}

pub(crate) const MAX_NODE_SCORE: u64 = 100;

pub(crate) trait Plugin {
    fn name(&self) -> &str;
}
//*
// NOTE: the plugin system is inspired by the Kubernetes scheduler plugin system.

// Copyright 2019 The Kubernetes Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
// */
pub trait QueueSort: Plugin {
    fn less(&self, pod1: &Pod, pod2: &Pod) -> bool;
}

// /// These plugins are used to pre-process info about the Pod, or to check certain conditions that
// /// the cluster or the Pod must meet. If a PreFilter plugin returns an error,
// /// the scheduling cycle is aborted
pub trait PreFilter {
    fn pre_filter(&self, pod: &Pod, nodes: &[NodeState]) -> Result<(), Box<dyn Error>>;
}

/// These plugins are used to filter out nodes that cannot run the Pod. For each node, the scheduler
/// will call filter plugins in their configured order. If any filter plugin marks the node as
/// infeasible, the remaining plugins will not be called for that node.
pub trait Filter: Plugin {
    fn filter(&self, pod: &Pod, node: &NodeState) -> Result<(), String>;
}
#[derive(thiserror::Error, Debug)]
pub enum ScoreError {
    #[error("pod spec is empty")]
    PodSpecEmpty,
    #[error("{0}")]
    PodHelper(#[from] pod_helpers::Error),
}

/// These plugins are used to rank nodes that have passed the filtering phase. The scheduler will
/// call each scoring plugin for each node. There will be a well defined range of integers
/// representing the minimum and maximum scores. After the NormalizeScore phase, the scheduler will
/// combine node scores from all plugins according to the configured plugin weights.
pub trait Score: Plugin {
    fn score(&self, pod: &Pod, node: &NodeState) -> Result<u64, ScoreError>;

    /// These plugins are used to modify node scores before the scheduler computes a final ranking of Nodes.
    /// A plugin that registers for this extension point will be called with the Score results from the
    /// same plugin.
    fn normalize_scores(&self, scores: &mut BTreeMap<String, u64>) -> Result<(), ScoreError> {
        if scores.is_empty() {
            return Ok(());
        }
        let values = scores.values().cloned();
        let (min, max) = values.minmax().into_option().unwrap_or((0, 0));
        for (_, score) in scores.iter_mut() {
            if max == 0 || min == max {
                *score = MAX_NODE_SCORE;
                continue;
            }

            // Normalize the score to a range of 0 to MAX_NODE_SCORE
            *score = (*score - min) * MAX_NODE_SCORE / (max - min)
        }

        Ok(())
    }
}

pub(crate) fn inverted_normalize_scores(
    scores: &mut BTreeMap<String, u64>,
) -> Result<(), ScoreError> {
    if scores.is_empty() {
        return Ok(());
    }
    let values = scores.values().cloned();
    let (min, max) = values.minmax().into_option().unwrap_or((0, 0));
    for (_, score) in scores.iter_mut() {
        if max == 0 || min == max {
            *score = MAX_NODE_SCORE;
            continue;
        }
        // Invert the normalized score to a range of 0 to MAX_NODE_SCORE
        *score = MAX_NODE_SCORE - (*score - min) * MAX_NODE_SCORE / (max - min);
    }

    Ok(())
}

/// This plugin is called before the scheduler binds the Pod to a Node.
/// It can be used to perform any final setup on the Node prior to binding, like setting up a
/// network interface or preparing a volume
pub trait PreBind: Plugin {
    fn pre_bind(&self, pod: &Pod, node: &NodeState) -> Result<(), Box<dyn Error>>;
}

pub trait PostBind: Plugin {
    fn post_bind(&self, pod: &Pod, node: &NodeState) -> Result<(), Box<dyn Error>>;
}

mod tests {
    use crate::scheduler::plugins::{Plugin, ScoreError};
    use std::collections::BTreeMap;
    use std::error::Error;

    struct TestScore {}

    impl Plugin for TestScore {
        fn name(&self) -> &'static str {
            "test-score"
        }
    }

    impl super::Score for TestScore {
        fn score(
            &self,
            _pod: &k8s_openapi::api::core::v1::Pod,
            _node: &super::NodeState,
        ) -> Result<u64, ScoreError> {
            Ok(0)
        }
    }
    #[test]
    fn test_normalize_scores() {
        use super::*;
        let mut scores: BTreeMap<String, u64> = BTreeMap::new();
        scores.insert("node1".to_string(), 10);
        scores.insert("node2".to_string(), 50);
        scores.insert("node3".to_string(), 75);

        let mut plugin = TestScore {};
        plugin.normalize_scores(&mut scores).unwrap();

        assert_eq!(scores.get("node1").unwrap(), &0);
        assert_eq!(scores.get("node2").unwrap(), &61);
        assert_eq!(scores.get("node3").unwrap(), &100);
    }
}
