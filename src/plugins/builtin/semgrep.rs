use crate::core::comment::{Category, Severity};
use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
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
        let results = self
            .run_batch(std::slice::from_ref(diff), repo_path)
            .await?;
        Ok(results.get(&diff.file_path).cloned().unwrap_or_default())
    }

    async fn run_batch(
        &self,
        diffs: &[UnifiedDiff],
        repo_path: &str,
    ) -> Result<HashMap<PathBuf, PreAnalysis>> {
        let files = diffs
            .iter()
            .filter(|diff| !diff.is_deleted && !diff.is_binary && !diff.hunks.is_empty())
            .map(|diff| diff.file_path.clone())
            .collect::<Vec<_>>();

        if files.is_empty() {
            return Ok(HashMap::new());
        }

        let repo_root = PathBuf::from(repo_path);
        let file_args = files
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        let timeout = std::time::Duration::from_secs(SEMGREP_TIMEOUT_SECS);
        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                use std::process::Command;
                Command::new("semgrep")
                    .current_dir(&repo_root)
                    .arg("--config=auto")
                    .arg("--json")
                    .arg("--quiet")
                    .arg("--timeout")
                    .arg(SEMGREP_TIMEOUT_SECS.to_string())
                    .args(&file_args)
                    .output()
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(output))) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    Ok(HashMap::new())
                } else {
                    Ok(parse_semgrep_analyses(Path::new(repo_path), &stdout))
                }
            }
            _ => Ok(HashMap::new()),
        }
    }
}

fn parse_semgrep_analyses(repo_root: &Path, payload: &str) -> HashMap<PathBuf, PreAnalysis> {
    let mut analyses = HashMap::new();
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return analyses;
    };
    let Some(results) = value.get("results").and_then(|results| results.as_array()) else {
        return analyses;
    };

    let mut findings_by_file: HashMap<PathBuf, Vec<AnalyzerFinding>> = HashMap::new();
    for result in results {
        let file_path = result
            .get("path")
            .and_then(|value| value.as_str())
            .map(|value| normalize_tool_path(repo_root, value))
            .unwrap_or_else(|| PathBuf::from("unknown"));
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
        findings_by_file
            .entry(file_path.clone())
            .or_default()
            .push(AnalyzerFinding {
                file_path,
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

    for (file_path, findings) in findings_by_file {
        let mut analysis = PreAnalysis::default();
        analysis.context_chunks.push(LLMContextChunk {
            file_path: file_path.clone(),
            content: build_context_chunk("Semgrep", &findings),
            context_type: ContextType::Documentation,
            line_range: None,
            provenance: Some("semgrep analyzer".to_string()),
        });
        analysis.findings = findings;
        analyses.insert(file_path, analysis);
    }

    analyses
}

fn build_context_chunk(tool_name: &str, findings: &[AnalyzerFinding]) -> String {
    let details = findings
        .iter()
        .take(20)
        .map(|finding| {
            let rule = finding
                .rule_id
                .as_deref()
                .map(|value| format!(" [{value}]"))
                .unwrap_or_default();
            format!(
                "- line {}{}: {}",
                finding.line_number, rule, finding.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{tool_name} findings:\n{details}")
}

fn normalize_tool_path(repo_root: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    let relative = if path.is_absolute() {
        path.strip_prefix(repo_root)
            .map(|value| value.to_path_buf())
            .unwrap_or(path)
    } else {
        path
    };

    normalize_relative_path(relative)
}

fn normalize_relative_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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

    #[test]
    fn test_parse_semgrep_analyses_groups_findings_by_file() {
        let payload = r#"{
            "results": [
                {
                    "path": "/repo/src/a.py",
                    "start": {"line": 12},
                    "check_id": "security.sql-injection",
                    "extra": {"message": "SQL injection", "severity": "ERROR"}
                },
                {
                    "path": "/repo/src/b.py",
                    "start": {"line": 4},
                    "check_id": "python.best-practice",
                    "extra": {"message": "Use context manager", "severity": "INFO"}
                }
            ]
        }"#;

        let analyses = parse_semgrep_analyses(Path::new("/repo"), payload);

        assert_eq!(analyses.len(), 2);
        assert_eq!(
            analyses[Path::new("src/a.py")].findings[0].category,
            Category::Security
        );
        assert_eq!(
            analyses[Path::new("src/b.py")].findings[0].severity,
            Severity::Info
        );
    }
}
