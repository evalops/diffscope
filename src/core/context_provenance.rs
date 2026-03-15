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
    DocumentContext {
        source: String,
        title: String,
    },
    JiraIssueContext {
        issue_key: String,
    },
    LinearIssueContext {
        issue_id: String,
    },
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
    SimilarImplementation {
        similarity: f32,
        symbol_name: String,
    },
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

    pub fn jira_issue_context(issue_key: impl Into<String>) -> Self {
        Self::JiraIssueContext {
            issue_key: issue_key.into(),
        }
    }

    pub fn document_context(source: impl Into<String>, title: impl Into<String>) -> Self {
        Self::DocumentContext {
            source: source.into(),
            title: title.into(),
        }
    }

    pub fn linear_issue_context(issue_id: impl Into<String>) -> Self {
        Self::LinearIssueContext {
            issue_id: issue_id.into(),
        }
    }

    pub fn semantic_retrieval(similarity: f32, symbol_name: impl Into<String>) -> Self {
        Self::SemanticRetrieval {
            similarity,
            symbol_name: symbol_name.into(),
        }
    }

    pub fn similar_implementation(similarity: f32, symbol_name: impl Into<String>) -> Self {
        Self::SimilarImplementation {
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
            Self::SimilarImplementation { .. } => 30,
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
            | Self::DocumentContext { .. }
            | Self::JiraIssueContext { .. }
            | Self::LinearIssueContext { .. }
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
            Self::SimilarImplementation { .. } => 35,
            Self::SemanticRetrieval { .. } => 30,
            _ => 0,
        }
    }

    pub fn artifact_tag(&self) -> Option<String> {
        let source = match self {
            Self::ActiveReviewRules | Self::Analyzer { .. } => return None,
            Self::CustomContextNotes => "custom-context".to_string(),
            Self::DocumentContext { source, .. } => source.trim().to_string(),
            Self::JiraIssueContext { .. } => "jira-issue".to_string(),
            Self::LinearIssueContext { .. } => "linear-issue".to_string(),
            Self::DependencyGraphNeighborhood => "dependency-graph".to_string(),
            Self::PathSpecificFocusAreas => "path-focus".to_string(),
            Self::RepositoryGraphMetadata => "repository-graph".to_string(),
            Self::PatternRepositoryContext { source }
            | Self::PatternRepositorySource { source } => {
                format!("pattern-repository:{}", source.trim())
            }
            Self::RelatedTestFile => "related-test-file".to_string(),
            Self::ReverseDependencySummary => "reverse-dependency-summary".to_string(),
            Self::SimilarImplementation { .. } => "similar-implementation".to_string(),
            Self::SemanticRetrieval { .. } => "semantic-retrieval".to_string(),
            Self::SymbolGraphPath { .. } => "symbol-graph".to_string(),
        };

        Some(format!("context-source:{source}"))
    }

    fn label(&self) -> String {
        match self {
            Self::ActiveReviewRules => "active review rules".to_string(),
            Self::Analyzer { name } => format!("{name} analyzer"),
            Self::CustomContextNotes => "custom context notes".to_string(),
            Self::DocumentContext { source, title } => {
                format!("document context ({source}): {title}")
            }
            Self::JiraIssueContext { issue_key } => format!("jira issue context: {issue_key}"),
            Self::LinearIssueContext { issue_id } => {
                format!("linear issue context: {issue_id}")
            }
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
            Self::SimilarImplementation {
                similarity,
                symbol_name,
            } => {
                format!("similar implementation (similarity={similarity:.2}, symbol={symbol_name})")
            }
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

        let similar = ContextProvenance::similar_implementation(0.91, "validate_user");
        assert_eq!(
            similar.to_string(),
            "similar implementation (similarity=0.91, symbol=validate_user)"
        );
        assert_eq!(similar.ranking_bonus(), 30);
        assert_eq!(similar.verification_bonus(), 35);
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
        assert_eq!(
            ContextProvenance::document_context("design-doc", "Checkout architecture").to_string(),
            "document context (design-doc): Checkout architecture"
        );
        assert_eq!(
            ContextProvenance::jira_issue_context("ENG-123").to_string(),
            "jira issue context: ENG-123"
        );
        assert_eq!(
            ContextProvenance::linear_issue_context("LIN-42").to_string(),
            "linear issue context: LIN-42"
        );
    }

    #[test]
    fn artifact_tags_are_stable_for_external_context_sources() {
        assert_eq!(
            ContextProvenance::CustomContextNotes
                .artifact_tag()
                .as_deref(),
            Some("context-source:custom-context")
        );
        assert_eq!(
            ContextProvenance::pattern_repository_source("Acme/security-rules")
                .artifact_tag()
                .as_deref(),
            Some("context-source:pattern-repository:Acme/security-rules")
        );
        assert_eq!(
            ContextProvenance::symbol_graph_path(vec!["calls".to_string()], 1, 0.7)
                .artifact_tag()
                .as_deref(),
            Some("context-source:symbol-graph")
        );
        assert_eq!(
            ContextProvenance::document_context("runbook", "Pager escalation")
                .artifact_tag()
                .as_deref(),
            Some("context-source:runbook")
        );
        assert_eq!(
            ContextProvenance::jira_issue_context("ENG-123")
                .artifact_tag()
                .as_deref(),
            Some("context-source:jira-issue")
        );
        assert_eq!(
            ContextProvenance::linear_issue_context("LIN-42")
                .artifact_tag()
                .as_deref(),
            Some("context-source:linear-issue")
        );
        assert!(ContextProvenance::ActiveReviewRules
            .artifact_tag()
            .is_none());
    }
}
