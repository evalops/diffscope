use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    pub file_path: PathBuf,
    pub line_number: usize,
    pub content: String,
    #[serde(default)]
    pub rule_id: Option<String>,
    pub severity: Severity,
    pub category: Category,
    pub suggestion: Option<String>,
    pub confidence: f32,
    pub code_suggestion: Option<CodeSuggestion>,
    pub tags: Vec<String>,
    pub fix_effort: FixEffort,
    #[serde(default)]
    pub feedback: Option<String>,
    #[serde(default)]
    pub status: CommentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSuggestion {
    pub original_code: String,
    pub suggested_code: String,
    pub explanation: String,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total_comments: usize,
    pub by_severity: HashMap<String, usize>,
    pub by_category: HashMap<String, usize>,
    pub critical_issues: usize,
    pub files_reviewed: usize,
    pub overall_score: f32,
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub open_comments: usize,
    #[serde(default)]
    pub open_by_severity: HashMap<String, usize>,
    #[serde(default)]
    pub open_blocking_comments: usize,
    #[serde(default)]
    pub open_informational_comments: usize,
    #[serde(default)]
    pub resolved_comments: usize,
    #[serde(default)]
    pub dismissed_comments: usize,
    #[serde(default)]
    pub open_blockers: usize,
    #[serde(default)]
    pub merge_readiness: MergeReadiness,
    #[serde(default)]
    pub verification: ReviewVerificationSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CommentStatus {
    #[default]
    Open,
    Resolved,
    Dismissed,
}

impl std::fmt::Display for CommentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommentStatus::Open => write!(f, "Open"),
            CommentStatus::Resolved => write!(f, "Resolved"),
            CommentStatus::Dismissed => write!(f, "Dismissed"),
        }
    }
}

impl CommentStatus {
    pub fn from_api_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "resolved" => Some(Self::Resolved),
            "dismissed" => Some(Self::Dismissed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MergeReadiness {
    Ready,
    NeedsAttention,
    #[default]
    NeedsReReview,
}

impl std::fmt::Display for MergeReadiness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeReadiness::Ready => write!(f, "Ready"),
            MergeReadiness::NeedsAttention => write!(f, "Needs attention"),
            MergeReadiness::NeedsReReview => write!(f, "Needs re-review"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReviewVerificationSummary {
    #[serde(default)]
    pub state: ReviewVerificationState,
    #[serde(default)]
    pub judge_count: usize,
    #[serde(default)]
    pub required_votes: usize,
    #[serde(default)]
    pub warning_count: usize,
    #[serde(default)]
    pub filtered_comments: usize,
    #[serde(default)]
    pub abstained_comments: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ReviewVerificationState {
    #[default]
    NotApplicable,
    Verified,
    Inconclusive,
}

impl std::fmt::Display for ReviewVerificationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewVerificationState::NotApplicable => write!(f, "Not applicable"),
            ReviewVerificationState::Verified => write!(f, "Verified"),
            ReviewVerificationState::Inconclusive => write!(f, "Inconclusive"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Suggestion,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "Error"),
            Severity::Warning => write!(f, "Warning"),
            Severity::Info => write!(f, "Info"),
            Severity::Suggestion => write!(f, "Suggestion"),
        }
    }
}

impl Severity {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Suggestion => "suggestion",
        }
    }

    pub fn is_blocking(&self) -> bool {
        matches!(self, Severity::Error | Severity::Warning)
    }

    pub fn is_informational(&self) -> bool {
        matches!(self, Severity::Info | Severity::Suggestion)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Category {
    Bug,
    Security,
    Performance,
    Style,
    Documentation,
    BestPractice,
    Maintainability,
    Testing,
    Architecture,
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Bug => write!(f, "Bug"),
            Category::Security => write!(f, "Security"),
            Category::Performance => write!(f, "Performance"),
            Category::Style => write!(f, "Style"),
            Category::Documentation => write!(f, "Documentation"),
            Category::BestPractice => write!(f, "BestPractice"),
            Category::Maintainability => write!(f, "Maintainability"),
            Category::Testing => write!(f, "Testing"),
            Category::Architecture => write!(f, "Architecture"),
        }
    }
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Bug => "bug",
            Category::Security => "security",
            Category::Performance => "performance",
            Category::Style => "style",
            Category::Documentation => "documentation",
            Category::BestPractice => "bestpractice",
            Category::Maintainability => "maintainability",
            Category::Testing => "testing",
            Category::Architecture => "architecture",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FixEffort {
    Low,    // < 5 minutes
    Medium, // 5-30 minutes
    High,   // > 30 minutes
}

#[derive(Debug)]
pub struct RawComment {
    pub file_path: PathBuf,
    pub line_number: usize,
    pub content: String,
    pub rule_id: Option<String>,
    pub suggestion: Option<String>,
    pub severity: Option<Severity>,
    pub category: Option<Category>,
    pub confidence: Option<f32>,
    pub fix_effort: Option<FixEffort>,
    pub tags: Vec<String>,
    pub code_suggestion: Option<CodeSuggestion>,
}
