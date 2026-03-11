use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::PreAnalyzer;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

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

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<Vec<LLMContextChunk>> {
        let path_str = diff.file_path.to_string_lossy();
        if !JS_EXTENSIONS.iter().any(|ext| path_str.ends_with(ext)) {
            return Ok(Vec::new());
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
                    Ok(vec![LLMContextChunk {
                        file_path: diff_file_path,
                        content: format!("ESLint analysis:\n{}", stdout),
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
            assert!(result.is_empty(), "Should skip {}", ext);
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
