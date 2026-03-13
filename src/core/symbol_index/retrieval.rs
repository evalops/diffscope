use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::core::symbol_graph::RankedSymbol;
use crate::core::ContextProvenance;

use super::{SymbolIndex, SymbolLocation};

#[derive(Debug, Clone, Copy)]
pub struct SymbolRetrievalPolicy {
    pub max_locations: usize,
    pub graph_hops: usize,
    pub graph_max_files: usize,
}

impl SymbolRetrievalPolicy {
    pub fn new(max_locations: usize, graph_hops: usize, graph_max_files: usize) -> Self {
        Self {
            max_locations,
            graph_hops,
            graph_max_files,
        }
    }
}

#[derive(Debug, Default)]
pub struct RelatedSymbolLocations {
    pub definition_locations: Vec<SymbolLocation>,
    pub reference_locations: Vec<SymbolLocation>,
}

pub struct SymbolContextRetriever<'a> {
    index: &'a SymbolIndex,
    policy: SymbolRetrievalPolicy,
}

impl<'a> SymbolContextRetriever<'a> {
    pub fn new(index: &'a SymbolIndex, policy: SymbolRetrievalPolicy) -> Self {
        Self { index, policy }
    }

    pub fn related_symbol_locations(
        &self,
        current_file: &Path,
        symbols: &[String],
    ) -> RelatedSymbolLocations {
        RelatedSymbolLocations {
            definition_locations: self.graph_related_locations(current_file, symbols),
            reference_locations: self.multi_hop_locations(current_file, symbols),
        }
    }

    fn graph_related_locations(
        &self,
        current_file: &Path,
        symbols: &[String],
    ) -> Vec<SymbolLocation> {
        let Some(graph) = &self.index.symbol_graph else {
            return Vec::new();
        };
        if symbols.is_empty()
            || self.policy.max_locations == 0
            || self.policy.graph_max_files == 0
            || self.policy.graph_hops == 0
        {
            return Vec::new();
        }

        let ranked = graph.related_symbols(
            symbols,
            self.policy.graph_hops,
            self.policy
                .max_locations
                .saturating_mul(self.policy.graph_max_files)
                .max(self.policy.max_locations),
        );

        let mut results = Vec::new();
        let mut seen_locations = HashSet::new();
        let mut seen_files = HashSet::new();

        for ranked_symbol in ranked {
            if ranked_symbol.file_path == current_file {
                continue;
            }
            if seen_files.len() >= self.policy.graph_max_files
                && !seen_files.contains(&ranked_symbol.file_path)
            {
                continue;
            }

            let Some(mut location) = self.lookup_ranked_symbol_location(&ranked_symbol) else {
                continue;
            };

            let location_key = format!(
                "{}:{}:{}",
                location.file_path.display(),
                location.line_range.0,
                location.line_range.1
            );
            if !seen_locations.insert(location_key) {
                continue;
            }

            let relation_path = ranked_symbol
                .relation_path
                .iter()
                .map(|relation| relation.as_label().to_string())
                .collect::<Vec<_>>();
            location.provenance = Some(ContextProvenance::symbol_graph_path(
                relation_path.clone(),
                ranked_symbol.hops,
                ranked_symbol.relevance_score,
            ));
            location.snippet = format!(
                "[Graph: {}, hops={}, relevance={:.2}]\n{}",
                relation_path.join(" -> "),
                ranked_symbol.hops,
                ranked_symbol.relevance_score,
                location.snippet
            );
            seen_files.insert(location.file_path.clone());
            results.push(location);
        }

        results
    }

    fn multi_hop_locations(&self, current_file: &Path, symbols: &[String]) -> Vec<SymbolLocation> {
        if symbols.is_empty() || self.policy.graph_max_files == 0 {
            return Vec::new();
        }

        let mut direct_files = HashSet::new();
        let mut locations = Vec::new();
        let mut seen_locations = HashSet::new();

        for symbol in symbols {
            if let Some(entries) = self.index.lookup(symbol) {
                for location in entries.iter().take(self.policy.max_locations) {
                    let location_key = format!(
                        "{}:{}:{}",
                        location.file_path.display(),
                        location.line_range.0,
                        location.line_range.1
                    );
                    if seen_locations.insert(location_key) {
                        direct_files.insert(location.file_path.clone());
                        locations.push(location.clone());
                    }
                }
            }
        }

        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        let mut seen_files = HashSet::new();

        for file in direct_files {
            if file == current_file {
                continue;
            }
            seen_files.insert(file.clone());
            queue.push_back((file, 0));
        }

        while let Some((file, depth)) = queue.pop_front() {
            if depth >= self.policy.graph_hops {
                continue;
            }

            for neighbor in self.neighbor_files(&file) {
                if neighbor == current_file {
                    continue;
                }
                if !seen_files.insert(neighbor.clone()) {
                    continue;
                }
                queue.push_back((neighbor, depth + 1));
            }
        }

        for file in seen_files.into_iter().take(self.policy.graph_max_files) {
            if locations.iter().any(|location| location.file_path == file) {
                continue;
            }
            if let Some(summary) = self.index.file_summaries.get(&file) {
                locations.push(SymbolLocation {
                    file_path: file,
                    line_range: (1, summary.line_count.max(1)),
                    snippet: format!("[Dependency graph context]\n{}", summary.snippet),
                    provenance: Some(ContextProvenance::DependencyGraphNeighborhood),
                });
            }
        }

        locations
    }

    fn neighbor_files(&self, file: &Path) -> HashSet<PathBuf> {
        let mut neighbors = HashSet::new();
        if let Some(deps) = self.index.dependency_graph.get(file) {
            neighbors.extend(deps.iter().cloned());
        }
        if let Some(reverse) = self.index.reverse_dependency_graph.get(file) {
            neighbors.extend(reverse.iter().cloned());
        }
        neighbors
    }

    fn lookup_ranked_symbol_location(&self, ranked: &RankedSymbol) -> Option<SymbolLocation> {
        self.index.lookup(&ranked.name).and_then(|locations| {
            locations
                .iter()
                .filter(|location| location.file_path == ranked.file_path)
                .min_by_key(|location| {
                    let start = location.line_range.0;
                    let end = location.line_range.1;
                    if ranked.line < start {
                        start - ranked.line
                    } else {
                        ranked.line.saturating_sub(end)
                    }
                })
                .cloned()
        })
    }
}
