use crate::core::Comment;
use crate::plugins::PostProcessor;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;

pub struct DuplicateFilter;

impl DuplicateFilter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PostProcessor for DuplicateFilter {
    fn id(&self) -> &str {
        "duplicate_filter"
    }

    async fn run(&self, mut comments: Vec<Comment>, _repo_path: &str) -> Result<Vec<Comment>> {
        let mut seen = HashSet::new();
        comments.retain(|comment| {
            let key = format!(
                "{}:{}:{}",
                comment.file_path.display(),
                comment.line_number,
                comment.content
            );
            seen.insert(key)
        });

        Ok(comments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    fn make_comment(file: &str, line: usize, content: &str) -> Comment {
        Comment {
            id: format!("c-{file}-{line}"),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Style,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn test_duplicate_filter_id() {
        let filter = DuplicateFilter::new();
        assert_eq!(filter.id(), "duplicate_filter");
    }

    #[tokio::test]
    async fn test_removes_exact_duplicates() {
        let filter = DuplicateFilter::new();
        let comments = vec![
            make_comment("a.rs", 10, "fix this"),
            make_comment("a.rs", 10, "fix this"),
            make_comment("a.rs", 10, "fix this"),
        ];
        let result = filter.run(comments, "/repo").await.unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_keeps_different_lines() {
        let filter = DuplicateFilter::new();
        let comments = vec![
            make_comment("a.rs", 10, "fix this"),
            make_comment("a.rs", 20, "fix this"),
        ];
        let result = filter.run(comments, "/repo").await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_keeps_different_content() {
        let filter = DuplicateFilter::new();
        let comments = vec![
            make_comment("a.rs", 10, "fix this"),
            make_comment("a.rs", 10, "fix that"),
        ];
        let result = filter.run(comments, "/repo").await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_keeps_different_files() {
        let filter = DuplicateFilter::new();
        let comments = vec![
            make_comment("a.rs", 10, "fix this"),
            make_comment("b.rs", 10, "fix this"),
        ];
        let result = filter.run(comments, "/repo").await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_empty_input() {
        let filter = DuplicateFilter::new();
        let result = filter.run(vec![], "/repo").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_preserves_order() {
        let filter = DuplicateFilter::new();
        let comments = vec![
            make_comment("c.rs", 1, "third"),
            make_comment("a.rs", 1, "first"),
            make_comment("b.rs", 1, "second"),
            make_comment("a.rs", 1, "first"), // duplicate
        ];
        let result = filter.run(comments, "/repo").await.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content, "third");
        assert_eq!(result[1].content, "first");
        assert_eq!(result[2].content, "second");
    }
}
