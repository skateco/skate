use crate::state::state::NodeState;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::ops::DerefMut;

const MAX_NODE_SCORE: u32 = 100;
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
fn pod_priority(pod: &Pod) -> i32 {
    pod.spec
        .as_ref()
        .and_then(|spec| spec.priority)
        .unwrap_or(0)
}
pub trait QueueSort {
    fn less(pod1: &Pod, pod2: &Pod) -> bool {
        let p1 = pod_priority(pod1);
        let p2 = pod_priority(pod2);
        // k8s orders earlier pods first, but we don't have that info
        p1 > p2
    }
}

/// These plugins are used to pre-process info about the Pod, or to check certain conditions that
/// the cluster or the Pod must meet. If a PreFilter plugin returns an error,
/// the scheduling cycle is aborted
pub trait PreFilter {
    fn pre_filter(&self, pod: &Pod, nodes: &[NodeState]) -> Result<(), Box<dyn Error>>;
}

/// These plugins are used to filter out nodes that cannot run the Pod. For each node, the scheduler
/// will call filter plugins in their configured order. If any filter plugin marks the node as
/// infeasible, the remaining plugins will not be called for that node.
pub trait Filter {
    fn filter(&self, pod: &Pod, node: &NodeState) -> Result<(), String>;
}

/// These plugins are used to rank nodes that have passed the filtering phase. The scheduler will
/// call each scoring plugin for each node. There will be a well defined range of integers
/// representing the minimum and maximum scores. After the NormalizeScore phase, the scheduler will
/// combine node scores from all plugins according to the configured plugin weights.
pub trait Score {
    fn score(&self, pod: &Pod, node: &NodeState) -> Result<u32, Box<dyn Error>>;

    /// These plugins are used to modify node scores before the scheduler computes a final ranking of Nodes.
    /// A plugin that registers for this extension point will be called with the Score results from the
    /// same plugin.
    fn normalize_scores(
        &self,
        mut scores: &mut BTreeMap<String, u32>,
    ) -> Result<(), Box<dyn Error>> {
        if scores.is_empty() {
            return Ok(());
        }
        let values = scores.values().cloned();
        let (min, max) = values.minmax().into_option().unwrap_or((0, 0));
        for (_, score) in scores.iter_mut() {
            if max == 0 {
                *score = MAX_NODE_SCORE;
                continue;
            }
            *score = MAX_NODE_SCORE * (max + min - *score) / max;
        }

        Ok(())
    }
}

/// This plugin is called before the scheduler binds the Pod to a Node.
/// It can be used to perform any final setup on the Node prior to binding, like setting up a
/// network interface or preparing a volume
pub trait PreBind {
    fn pre_bind(&self, pod: &Pod, node: &NodeState) -> Result<(), Box<dyn Error>>;
}

pub trait PostBind {
    fn post_bind(&self, pod: &Pod, node: &NodeState) -> Result<(), Box<dyn Error>>;
}
