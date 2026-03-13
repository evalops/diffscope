use crate::core::comment::{Category, Severity};
use crate::core::{LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
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
            .filter(|diff| {
                let path_str = diff.file_path.to_string_lossy();
                JS_EXTENSIONS.iter().any(|ext| path_str.ends_with(ext))
            })
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

        let timeout = std::time::Duration::from_secs(30);
        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                use std::process::Command;
                Command::new("eslint")
                    .current_dir(&repo_root)
                    .arg("--format=json")
                    .args(&file_args)
                    .output()
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(output))) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    return Ok(HashMap::new());
                }

                Ok(parse_eslint_analyses(Path::new(repo_path), &stdout))
            }
            _ => Ok(HashMap::new()),
        }
    }
}

fn parse_eslint_analyses(repo_root: &Path, payload: &str) -> HashMap<PathBuf, PreAnalysis> {
    let mut analyses = HashMap::new();
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return analyses;
    };
    let Some(files) = value.as_array() else {
        return analyses;
    };

    for file in files {
        let file_path = file
            .get("filePath")
            .and_then(|value| value.as_str())
            .map(|value| normalize_tool_path(repo_root, value))
            .unwrap_or_else(|| PathBuf::from("unknown"));
        let Some(messages) = file.get("messages").and_then(|value| value.as_array()) else {
            continue;
        };
        if messages.is_empty() {
            continue;
        }

        let mut findings = Vec::new();
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
                file_path: file_path.clone(),
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

        if !findings.is_empty() {
            let mut analysis = PreAnalysis::default();
            analysis.context_chunks.push(
                LLMContextChunk::documentation(
                    file_path.clone(),
                    build_context_chunk("ESLint", &findings),
                )
                .with_provenance(crate::core::ContextProvenance::analyzer("eslint")),
            );
            analysis.findings = findings;
            analyses.insert(file_path, analysis);
        }
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

    #[test]
    fn test_parse_eslint_analyses_groups_findings_by_file() {
        let payload = r#"[
            {
                "filePath": "/repo/src/app.ts",
                "messages": [
                    {"line": 3, "severity": 2, "ruleId": "no-eval", "message": "Avoid eval"}
                ]
            },
            {
                "filePath": "/repo/src/ui.tsx",
                "messages": [
                    {"line": 8, "severity": 1, "ruleId": "react/jsx-key", "message": "Missing key"}
                ]
            }
        ]"#;

        let analyses = parse_eslint_analyses(Path::new("/repo"), payload);

        assert_eq!(analyses.len(), 2);
        assert_eq!(
            analyses[Path::new("src/app.ts")].findings[0].severity,
            Severity::Warning
        );
        assert_eq!(
            analyses[Path::new("src/ui.tsx")].findings[0].content,
            "Missing key"
        );
    }
}
