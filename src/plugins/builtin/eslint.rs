use crate::core::comment::{Category, Severity};
use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct EslintAnalyzer;

impl EslintAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

const JS_EXTENSIONS: &[&str] = &[".js", ".ts", ".jsx", ".tsx"];

#[async_trait]
impl PreAnalyzer for EslintAnalyzer {
    fn id(&self) -> &str {
        "eslint"
    }

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis> {
        let path_str = diff.file_path.to_string_lossy();
        if !JS_EXTENSIONS.iter().any(|ext| path_str.ends_with(ext)) {
            return Ok(PreAnalysis::default());
        }

        let file_path = PathBuf::from(repo_path).join(&diff.file_path);
        let file_arg = file_path.to_string_lossy().to_string();
        let diff_file_path = diff.file_path.clone();

        let timeout = std::time::Duration::from_secs(30);
        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                use std::process::Command;
                Command::new("eslint")
                    .arg("--format=json")
                    .arg("--no-eslintrc")
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
                        content: format!("ESLint analysis:\n{}", stdout),
                        context_type: ContextType::Documentation,
                        line_range: None,
                    });
                    analysis
                        .findings
                        .extend(parse_eslint_findings(&diff_file_path, &stdout));
                    Ok(analysis)
                } else {
                    Ok(PreAnalysis::default())
                }
            }
            _ => Ok(PreAnalysis::default()),
        }
    }
}

fn parse_eslint_findings(file_path: &Path, payload: &str) -> Vec<AnalyzerFinding> {
    let mut findings = Vec::new();
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return findings;
    };
    let Some(files) = value.as_array() else {
        return findings;
    };

    for file in files {
        let Some(messages) = file.get("messages").and_then(|value| value.as_array()) else {
            continue;
        };
        for message in messages {
            let line_number = message
                .get("line")
                .and_then(|value| value.as_u64())
                .map(|value| value as usize)
                .unwrap_or(1);
            let severity = match message
                .get("severity")
                .and_then(|value| value.as_u64())
                .unwrap_or(1)
            {
                2 => Severity::Warning,
                _ => Severity::Info,
            };
            let rule_id = message
                .get("ruleId")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            let content = message
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("ESLint reported a JavaScript issue")
                .to_string();
            findings.push(AnalyzerFinding {
                file_path: file_path.to_path_buf(),
                line_number,
                content,
                rule_id,
                suggestion: None,
                severity,
                category: Category::Style,
                confidence: 0.95,
                source: "eslint".to_string(),
                tags: vec!["eslint".to_string()],
                metadata: Default::default(),
            });
        }
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
    fn test_eslint_analyzer_id() {
        let analyzer = EslintAnalyzer::new();
        assert_eq!(analyzer.id(), "eslint");
    }

    #[tokio::test]
    async fn test_eslint_skips_non_js_files() {
        let analyzer = EslintAnalyzer::new();
        for ext in &[".rs", ".py", ".go", ".java", ".rb", ".css", ".html"] {
            let diff = make_diff(&format!("file{}", ext));
            let result = analyzer.run(&diff, "/tmp").await.unwrap();
            assert!(
                result.context_chunks.is_empty() && result.findings.is_empty(),
                "Should skip {}",
                ext
            );
        }
    }

    #[test]
    fn test_js_extensions_filter() {
        // Verify JS_EXTENSIONS contains all expected extensions
        for ext in &[".js", ".ts", ".jsx", ".tsx"] {
            assert!(
                JS_EXTENSIONS.contains(ext),
                "JS_EXTENSIONS should contain {}",
                ext
            );
        }
        // And rejects non-JS extensions
        for ext in &[".rs", ".py", ".go"] {
            assert!(
                !JS_EXTENSIONS.contains(ext),
                "JS_EXTENSIONS should not contain {}",
                ext
            );
        }
    }

    #[tokio::test]
    async fn test_eslint_handles_missing_binary() {
        let analyzer = EslintAnalyzer::new();
        let diff = make_diff("test.js");
        let result = analyzer.run(&diff, "/nonexistent/repo").await;
        assert!(result.is_ok());
    }
}
