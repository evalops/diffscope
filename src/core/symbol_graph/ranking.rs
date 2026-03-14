use crate::core::symbol_index::SymbolLocation;
use crate::core::ContextProvenance;

use super::{RankedSymbol, SymbolGraph, SymbolNode, SymbolRelation};

impl SymbolGraph {
    pub fn ranked_to_locations(&self, ranked: &[RankedSymbol]) -> Vec<SymbolLocation> {
        let mut locations = Vec::new();
        for ranked_symbol in ranked {
            if let Some(node) = self.lookup_ranked_node(ranked_symbol) {
                let relation_path = relation_path_summary(&ranked_symbol.relation_path);
                locations.push(SymbolLocation {
                    file_path: node.file_path.clone(),
                    line_range: node.line_range,
                    snippet: format!(
                        "[Graph: {}, hops={}, relevance={:.2}]\n{}",
                        relation_path, ranked_symbol.hops, ranked_symbol.relevance_score, node.name
                    ),
                    provenance: Some(ContextProvenance::symbol_graph_path(
                        ranked_symbol
                            .relation_path
                            .iter()
                            .map(|relation| relation.as_label().to_string())
                            .collect(),
                        ranked_symbol.hops,
                        ranked_symbol.relevance_score,
                    )),
                });
            }
        }
        locations
    }

    fn lookup_ranked_node(&self, ranked: &RankedSymbol) -> Option<&SymbolNode> {
        self.nodes.get(&ranked.name).and_then(|nodes| {
            nodes
                .iter()
                .filter(|node| node.file_path == ranked.file_path)
                .min_by_key(|node| {
                    let start = node.line_range.0;
                    let end = node.line_range.1;
                    if ranked.line < start {
                        start - ranked.line
                    } else {
                        ranked.line.saturating_sub(end)
                    }
                })
        })
    }
}

fn relation_path_summary(path: &[SymbolRelation]) -> String {
    if path.is_empty() {
        return "seed".to_string();
    }

    path.iter()
        .map(SymbolRelation::as_label)
        .collect::<Vec<_>>()
        .join(" -> ")
}
