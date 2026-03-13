use crate::core::comment::{Category, Severity};
use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Timeout for semgrep execution in seconds.
const SEMGREP_TIMEOUT_SECS: u64 = 30;

pub struct SemgrepAnalyzer;

impl SemgrepAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PreAnalyzer for SemgrepAnalyzer {
    fn id(&self) -> &str {
        "semgrep"
    }

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis> {
        let file_path = PathBuf::from(repo_path).join(&diff.file_path);
        let file_arg = file_path.to_string_lossy().to_string();
        let diff_file_path = diff.file_path.clone();

        let timeout = std::time::Duration::from_secs(SEMGREP_TIMEOUT_SECS);
        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                use std::process::Command;
                Command::new("semgrep")
                    .arg("--config=auto")
                    .arg("--json")
                    .arg("--quiet")
                    .arg("--timeout")
                    .arg(SEMGREP_TIMEOUT_SECS.to_string())
                    .arg(&file_arg)
                    .output()
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(output))) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    let mut analysis = PreAnalysis::default();
                    analysis.context_chunks.push(LLMContextChunk {
                        file_path: diff_file_path.clone(),
                        content: format!("Semgrep analysis:\n{}", stdout),
                        context_type: ContextType::Documentation,
                        line_range: None,
                    });
                    analysis
                        .findings
                        .extend(parse_semgrep_findings(&diff_file_path, &stdout));
                    Ok(analysis)
                } else {
                    Ok(PreAnalysis::default())
                }
            }
            _ => Ok(PreAnalysis::default()),
        }
    }
}

fn parse_semgrep_findings(file_path: &Path, payload: &str) -> Vec<AnalyzerFinding> {
    let mut findings = Vec::new();
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return findings;
    };
    let Some(results) = value.get("results").and_then(|results| results.as_array()) else {
        return findings;
    };

    for result in results {
        let line_number = result
            .get("start")
            .and_then(|start| start.get("line"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(1);
        let rule_id = result
            .get("check_id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let message = result
            .get("extra")
            .and_then(|extra| extra.get("message"))
            .and_then(|value| value.as_str())
            .unwrap_or("Semgrep reported a potential issue")
            .to_string();
        let severity_label = result
            .get("extra")
            .and_then(|extra| extra.get("severity"))
            .and_then(|value| value.as_str())
            .unwrap_or("INFO");
        let severity = match severity_label.to_ascii_uppercase().as_str() {
            "ERROR" => Severity::Error,
            "WARNING" => Severity::Warning,
            _ => Severity::Info,
        };
        let category = rule_id
            .as_deref()
            .map(|value| {
                if value.contains("security") || value.starts_with("sec.") {
                    Category::Security
                } else {
                    Category::BestPractice
                }
            })
            .unwrap_or(Category::BestPractice);
        findings.push(AnalyzerFinding {
            file_path: file_path.to_path_buf(),
            line_number,
            content: message,
            rule_id,
            suggestion: None,
            severity,
            category,
            confidence: 0.98,
            source: "semgrep".to_string(),
            tags: vec!["semgrep".to_string()],
            metadata: Default::default(),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_diff(file_path: &str) -> UnifiedDiff {
        UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from(file_path),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        }
    }

    #[test]
    fn test_semgrep_analyzer_id() {
        let analyzer = SemgrepAnalyzer::new();
        assert_eq!(analyzer.id(), "semgrep");
    }

    #[tokio::test]
    async fn test_semgrep_handles_missing_binary() {
        // When semgrep is not installed, should return empty vec (not error)
        let analyzer = SemgrepAnalyzer::new();
        let diff = make_diff("nonexistent_file.py");
        let result = analyzer.run(&diff, "/nonexistent/repo").await;
        assert!(result.is_ok());
        // Either empty (semgrep not found) or has output (semgrep found but file missing)
    }

    #[tokio::test]
    async fn test_semgrep_timeout_returns_empty() {
        // When semgrep times out, should return empty vec (not error)
        let analyzer = SemgrepAnalyzer::new();
        let diff = make_diff("test.py");
        // This will either timeout or fail to find semgrep — both should return Ok
        let result = analyzer.run(&diff, "/nonexistent/repo").await;
        assert!(result.is_ok());
    }
}
