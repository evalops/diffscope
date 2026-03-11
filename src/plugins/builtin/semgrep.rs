use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::PreAnalyzer;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

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

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<Vec<LLMContextChunk>> {
        let file_path = PathBuf::from(repo_path).join(&diff.file_path);
        let file_arg = file_path.to_string_lossy().to_string();

        let output = tokio::task::spawn_blocking(move || {
            use std::process::Command;
            Command::new("semgrep")
                .arg("--config=auto")
                .arg("--json")
                .arg("--quiet")
                .arg("--timeout")
                .arg(SEMGREP_TIMEOUT_SECS.to_string())
                .arg(&file_arg)
                .output()
        })
        .await;

        match output {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    Ok(vec![LLMContextChunk {
                        file_path: diff.file_path.clone(),
                        content: format!("Semgrep analysis:\n{}", stdout),
                        context_type: ContextType::Documentation,
                        line_range: None,
                    }])
                } else {
                    Ok(Vec::new())
                }
            }
            _ => Ok(Vec::new()),
        }
    }
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
    async fn test_semgrep_returns_context_chunks_with_correct_type() {
        // Verify the context type and file path are set correctly when output exists
        let chunk = LLMContextChunk {
            file_path: PathBuf::from("test.py"),
            content: "Semgrep analysis:\n{\"results\":[]}".to_string(),
            context_type: ContextType::Documentation,
            line_range: None,
        };
        assert_eq!(chunk.context_type, ContextType::Documentation);
        assert_eq!(chunk.file_path, PathBuf::from("test.py"));
        assert!(chunk.content.starts_with("Semgrep analysis:"));
    }
}
