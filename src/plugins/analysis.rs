use crate::core::comment::{Category, RawComment, Severity};
use crate::core::LLMContextChunk;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreAnalysis {
    pub context_chunks: Vec<LLMContextChunk>,
    pub findings: Vec<AnalyzerFinding>,
}

impl PreAnalysis {
    pub fn extend(&mut self, other: PreAnalysis) {
        self.context_chunks.extend(other.context_chunks);
        self.findings.extend(other.findings);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerFinding {
    pub file_path: PathBuf,
    pub line_number: usize,
    pub content: String,
    pub rule_id: Option<String>,
    pub suggestion: Option<String>,
    pub severity: Severity,
    pub category: Category,
    pub confidence: f32,
    pub source: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl AnalyzerFinding {
    pub fn into_raw_comment(self) -> RawComment {
        let mut tags = self.tags;
        let source_tag = format!("source:{}", self.source);
        if !tags.iter().any(|tag| tag == &source_tag) {
            tags.push(source_tag);
        }

        RawComment {
            file_path: self.file_path,
            line_number: self.line_number,
            content: self.content,
            rule_id: self.rule_id,
            suggestion: self.suggestion,
            severity: Some(self.severity),
            category: Some(self.category),
            confidence: Some(self.confidence),
            fix_effort: None,
            tags,
            code_suggestion: None,
        }
    }
}
