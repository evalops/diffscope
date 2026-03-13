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
