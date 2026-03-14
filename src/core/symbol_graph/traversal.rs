use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::{NodeKey, RankedSymbol, SymbolGraph, SymbolNode, SymbolRelation};

#[derive(Debug, Clone)]
struct RankedState {
    cost: f32,
    file_path: PathBuf,
    line: usize,
    relation_path: Vec<SymbolRelation>,
    hops: usize,
}

impl SymbolGraph {
    pub fn related_symbols(
        &self,
        seed_symbols: &[String],
        max_hops: usize,
        max_results: usize,
    ) -> Vec<RankedSymbol> {
        if seed_symbols.is_empty() || max_results == 0 || max_hops == 0 {
            return Vec::new();
        }

        let mut seed_keys = HashSet::new();
        let mut best_states: HashMap<NodeKey, RankedState> = HashMap::new();
        let mut frontier: Vec<(NodeKey, usize, f32, Vec<SymbolRelation>)> = Vec::new();

        for seed in seed_symbols {
            if let Some(nodes) = self.nodes.get(seed) {
                for node in nodes {
                    let key = NodeKey::from_node(node);
                    seed_keys.insert(key.clone());
                    best_states.insert(
                        key.clone(),
                        RankedState {
                            cost: 0.0,
                            file_path: node.file_path.clone(),
                            line: node.line_range.0,
                            relation_path: Vec::new(),
                            hops: 0,
                        },
                    );
                    frontier.push((key, 0, 0.0, Vec::new()));
                }
            }
        }

        while !frontier.is_empty() {
            frontier.sort_by(|left, right| {
                left.2
                    .partial_cmp(&right.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.1.cmp(&right.1))
            });
            let (current, depth, accumulated_cost, relation_path) = frontier.remove(0);
            if depth >= max_hops {
                continue;
            }

            if let Some(node) = self.lookup_node(&current) {
                for edge in &node.edges {
                    let next_hops = depth + 1;
                    if next_hops > max_hops {
                        continue;
                    }

                    let next_cost =
                        accumulated_cost + edge.relation.relevance_weight() * next_hops as f32;
                    let mut next_path = relation_path.clone();
                    next_path.push(edge.relation.clone());
                    let next_key = NodeKey::from_edge(edge);

                    let should_update = best_states.get(&next_key).is_none_or(|existing| {
                        next_cost + f32::EPSILON < existing.cost
                            || ((next_cost - existing.cost).abs() <= f32::EPSILON
                                && next_hops < existing.hops)
                    });

                    if should_update {
                        best_states.insert(
                            next_key.clone(),
                            RankedState {
                                cost: next_cost,
                                file_path: edge.target_file.clone(),
                                line: edge.target_line,
                                relation_path: next_path.clone(),
                                hops: next_hops,
                            },
                        );
                        frontier.push((next_key, next_hops, next_cost, next_path));
                    }
                }
            }
        }

        let mut results = best_states
            .into_iter()
            .filter(|(key, state)| !seed_keys.contains(key) && !state.relation_path.is_empty())
            .map(|(key, state)| RankedSymbol {
                name: key.name,
                file_path: state.file_path,
                line: state.line,
                relevance_score: 1.0 / (1.0 + state.cost),
                relation_path: state.relation_path,
                hops: state.hops,
            })
            .collect::<Vec<_>>();

        results.sort_by(|left, right| {
            right
                .relevance_score
                .partial_cmp(&left.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);
        results
    }

    pub(super) fn lookup_node(&self, key: &NodeKey) -> Option<&SymbolNode> {
        self.nodes.get(&key.name).and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node.file_path == key.file_path && node.line_range.0 == key.line)
        })
    }
}
