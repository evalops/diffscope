use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextProvenance {
    ActiveReviewRules,
    Analyzer {
        name: String,
    },
    CustomContextNotes,
    DependencyGraphNeighborhood,
    PathSpecificFocusAreas,
    RepositoryGraphMetadata,
    PatternRepositoryContext {
        source: String,
    },
    PatternRepositorySource {
        source: String,
    },
    RelatedTestFile,
    ReverseDependencySummary,
    SemanticRetrieval {
        similarity: f32,
        symbol_name: String,
    },
    SymbolGraphPath {
        relation_path: Vec<String>,
        hops: usize,
        relevance: f32,
    },
}

impl ContextProvenance {
    pub fn analyzer(name: impl Into<String>) -> Self {
        Self::Analyzer { name: name.into() }
    }

    pub fn pattern_repository_context(source: impl Into<String>) -> Self {
        Self::PatternRepositoryContext {
            source: source.into(),
        }
    }

    pub fn pattern_repository_source(source: impl Into<String>) -> Self {
        Self::PatternRepositorySource {
            source: source.into(),
        }
    }

    pub fn semantic_retrieval(similarity: f32, symbol_name: impl Into<String>) -> Self {
        Self::SemanticRetrieval {
            similarity,
            symbol_name: symbol_name.into(),
        }
    }

    pub fn symbol_graph_path(relation_path: Vec<String>, hops: usize, relevance: f32) -> Self {
        Self::SymbolGraphPath {
            relation_path,
            hops,
            relevance,
        }
    }

    pub fn ranking_bonus(&self) -> i32 {
        match self {
            Self::ActiveReviewRules => 120,
            Self::PatternRepositorySource { .. } => 40,
            Self::PatternRepositoryContext { .. } => 35,
            Self::SemanticRetrieval { .. } => 25,
            Self::SymbolGraphPath {
                relation_path,
                hops,
                ..
            } => {
                let mut bonus = 50;
                if *hops == 1 {
                    bonus += 15;
                }
                if relation_path.iter().any(|step| {
                    step.eq_ignore_ascii_case("calls") || step.eq_ignore_ascii_case("called-by")
                }) {
                    bonus += 10;
                }
                bonus
            }
            Self::Analyzer { .. }
            | Self::CustomContextNotes
            | Self::DependencyGraphNeighborhood
            | Self::PathSpecificFocusAreas
            | Self::RepositoryGraphMetadata
            | Self::RelatedTestFile
            | Self::ReverseDependencySummary => 0,
        }
    }

    pub fn verification_bonus(&self) -> i32 {
        match self {
            Self::SymbolGraphPath { .. } => 80,
            Self::SemanticRetrieval { .. } => 30,
            _ => 0,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::ActiveReviewRules => "active review rules".to_string(),
            Self::Analyzer { name } => format!("{name} analyzer"),
            Self::CustomContextNotes => "custom context notes".to_string(),
            Self::DependencyGraphNeighborhood => "dependency graph neighborhood".to_string(),
            Self::PathSpecificFocusAreas => "path-specific focus areas".to_string(),
            Self::RepositoryGraphMetadata => "repository graph metadata".to_string(),
            Self::PatternRepositoryContext { source } => {
                format!("pattern repository: {source}")
            }
            Self::PatternRepositorySource { source } => {
                format!("pattern repository source: {source}")
            }
            Self::RelatedTestFile => "related test file".to_string(),
            Self::ReverseDependencySummary => "reverse dependency summary".to_string(),
            Self::SemanticRetrieval {
                similarity,
                symbol_name,
            } => format!("semantic retrieval (similarity={similarity:.2}, symbol={symbol_name})"),
            Self::SymbolGraphPath {
                relation_path,
                hops,
                relevance,
            } => format!(
                "symbol graph path: {} (hops={}, relevance={:.2})",
                if relation_path.is_empty() {
                    "seed".to_string()
                } else {
                    relation_path.join(" -> ")
                },
                hops,
                relevance
            ),
        }
    }
}

impl fmt::Display for ContextProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::ContextProvenance;

    #[test]
    fn symbol_graph_path_formats_and_scores() {
        let provenance = ContextProvenance::symbol_graph_path(
            vec!["calls".to_string(), "uses".to_string()],
            1,
            0.42,
        );

        assert_eq!(
            provenance.to_string(),
            "symbol graph path: calls -> uses (hops=1, relevance=0.42)"
        );
        assert_eq!(provenance.ranking_bonus(), 75);
        assert_eq!(provenance.verification_bonus(), 80);
    }

    #[test]
    fn active_rules_and_pattern_repository_have_stable_labels() {
        assert_eq!(
            ContextProvenance::ActiveReviewRules.to_string(),
            "active review rules"
        );
        assert_eq!(
            ContextProvenance::pattern_repository_source("org/repo").to_string(),
            "pattern repository source: org/repo"
        );
        assert_eq!(
            ContextProvenance::pattern_repository_context("org/repo").to_string(),
            "pattern repository: org/repo"
        );
        assert_eq!(
            ContextProvenance::RepositoryGraphMetadata.to_string(),
            "repository graph metadata"
        );
    }
}
