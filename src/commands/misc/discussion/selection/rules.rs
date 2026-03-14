use anyhow::Result;

use crate::core;

pub(in super::super) fn select_discussion_comment(
    comments: &[core::Comment],
    comment_id: Option<String>,
    comment_index: Option<usize>,
) -> Result<core::Comment> {
    if comment_id.is_some() && comment_index.is_some() {
        anyhow::bail!("Specify only one of --comment-id or --comment-index");
    }

    if let Some(id) = comment_id {
        let selected = comments
            .iter()
            .find(|comment| comment.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment id not found: {}", id))?;
        return Ok(selected);
    }

    if let Some(index) = comment_index {
        if index == 0 {
            anyhow::bail!("comment-index is 1-based");
        }
        let selected = comments
            .get(index - 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment index out of range: {}", index))?;
        return Ok(selected);
    }

    comments
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No comments available"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_select_discussion_comment_empty_comments() {
        let result = select_discussion_comment(&[], None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_discussion_comment_defaults_to_first() {
        let comment = core::Comment {
            id: "cmt_1".to_string(),
            file_path: PathBuf::from("test.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: core::comment::Severity::Info,
            category: core::comment::Category::BestPractice,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
        };
        let result = select_discussion_comment(std::slice::from_ref(&comment), None, None).unwrap();
        assert_eq!(result.id, "cmt_1");
    }
}
