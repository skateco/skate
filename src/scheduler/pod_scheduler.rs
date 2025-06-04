use crate::scheduler::filter::NodeSelectorFilter;
use crate::scheduler::least_pods::LeastPods;
use crate::scheduler::node_name::NodeNameFilter;
use crate::scheduler::node_resources_fit::{NodeResourcesFit, Strategy};
use crate::scheduler::plugins::{Filter, PreFilter, QueueSort, Score};
use crate::scheduler::priority_sort::PrioritySort;
use crate::scheduler::resource_allocation::LeastAllocated;
use crate::scheduler::unschedulable::UnschedulableFilter;
use crate::scheduler::{NodeSelection, RejectedNode};
use crate::state::state::NodeState;
use itertools::{Either, Itertools};
use k8s_openapi::api::core::v1::Pod;
use rand::seq::IteratorRandom;
use std::collections::BTreeMap;

pub const DEFAULT_MILLI_CPU_REQUEST: u64 = 100; // 0.1 CPU in milliCPU
pub const DEFAULT_MEMORY_REQUEST: u64 = 200 * 1024 * 1024; // 200 MiB in bytes

pub struct PodScheduler {
    sorter: Box<dyn QueueSort>,
    pre_filters: Vec<Box<dyn PreFilter>>,
    filters: Vec<Box<dyn Filter>>,
    scorers: Vec<Box<dyn Score>>,
}

impl PodScheduler {
    pub fn new() -> Self {
        Self {
            sorter: Box::new(PrioritySort {}),
            pre_filters: vec![Box::new(NodeResourcesFit::new(Strategy::LeastAllocated))],
            filters: vec![
                Box::new(NodeNameFilter {}),
                Box::new(NodeSelectorFilter {}),
                Box::new(UnschedulableFilter {}),
                Box::new(NodeResourcesFit::new(Strategy::LeastAllocated)),
            ],
            scorers: vec![
                Box::new(LeastPods {}),
                Box::new(NodeResourcesFit::new(Strategy::LeastAllocated)),
            ],
        }
    }
    pub fn choose_node(&self, nodes: &[NodeState], pod: &Pod) -> NodeSelection {
        for pre_filter in &self.pre_filters {
            if let Err(e) = pre_filter.pre_filter(pod, nodes) {
                return NodeSelection {
                    selected: None,
                    rejected: vec![RejectedNode {
                        node_name: "*".to_string(),
                        reason: e.to_string(),
                    }],
                };
            }
        }

        let (filtered_nodes, rejected_nodes): (Vec<_>, Vec<_>) = nodes.iter().partition_map(|n| {
            // apply all filters
            for filter in &self.filters {
                if let Err(e) = filter.filter(pod, n) {
                    return Either::Right(RejectedNode {
                        node_name: n.node_name.clone(),
                        reason: e,
                    });
                }
            }

            // if all filters pass, return the node
            Either::Left(n)
        });
        log::debug!(
            "filtered nodes: {:?}",
            filtered_nodes
                .iter()
                .map(|n| n.node_name.clone())
                .collect::<Vec<_>>()
        );
        log::debug!("rejected nodes: {:?}", rejected_nodes);

        if filtered_nodes.is_empty() {
            return NodeSelection {
                selected: None,
                rejected: rejected_nodes,
            };
        }

        let mut node_score_total = BTreeMap::<String, u64>::new();

        for scorer in &self.scorers {
            let mut scored_nodes = BTreeMap::<String, u64>::new();
            for node in &filtered_nodes {
                match scorer.score(pod, node) {
                    Err(e) => {
                        log::error!(
                            "{} failed to score node {} for pod {}: {}",
                            scorer.name(),
                            node.node_name,
                            pod.metadata.name.as_deref().unwrap_or("unknown"),
                            e
                        );
                        return NodeSelection {
                            selected: None,
                            rejected: vec![RejectedNode {
                                node_name: node.node_name.clone(),
                                reason: e.to_string(),
                            }],
                        };
                    }
                    Ok(score) => {
                        scored_nodes.insert(node.node_name.clone(), score);
                    }
                };
            }
            if let Err(e) = scorer.normalize_scores(&mut scored_nodes) {
                return NodeSelection {
                    selected: None,
                    rejected: vec![RejectedNode {
                        node_name: "*".to_string(),
                        reason: e.to_string(),
                    }],
                };
            }

            for (node_name, score) in scored_nodes {
                log::debug!(
                    "scorer {} scored node {} with {}",
                    scorer.name(),
                    node_name,
                    score
                );
                let total_score = node_score_total.entry(node_name).or_insert(0);
                *total_score += score;
            }
        }

        log::info!(
            "Node scores:\n{}",
            node_score_total
                .iter()
                .sorted_by(|a, b| b.1.cmp(a.1))
                .map(|(k, v)| format!("    | {} | {} |", &k, v))
                .join("\n")
        );

        let mut winners = vec![];
        let mut max_score = 0;
        for (node_name, score) in &node_score_total {
            if *score > max_score {
                winners.clear();
                max_score = *score;
            }
            if *score == max_score {
                winners.push(node_name.clone());
            }
        }

        let winner = if winners.len() > 1 {
            log::debug!("multiple nodes with max score, performing tie-breaker");

            // do a tie-breaker by random choice
            winners.iter().choose(&mut rand::rng()).unwrap()
        } else if winners.len() == 1 {
            winners.first().unwrap()
        } else {
            eprintln!("no nodes with max score found, this should not happen");
            return NodeSelection {
                selected: None,
                rejected: vec![RejectedNode {
                    node_name: "*".to_string(),
                    reason: "No nodes with max score found".to_string(),
                }],
            };
        };

        NodeSelection {
            selected: Some(
                nodes
                    .iter()
                    .find(|n| n.node_name == *winner)
                    .unwrap()
                    .clone(),
            ),
            rejected: rejected_nodes,
        }
    }
}
