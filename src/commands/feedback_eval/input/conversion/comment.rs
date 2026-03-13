use crate::core;
use crate::review;

use super::super::super::FeedbackEvalComment;
use super::labels::normalize_feedback_label;

pub(in super::super) fn feedback_comment_from_comment(
    source_kind: &str,
    review_id: Option<String>,
    repo: Option<String>,
    pr_number: Option<u32>,
    title: Option<String>,
    comment: core::Comment,
) -> Option<FeedbackEvalComment> {
    let accepted = normalize_feedback_label(comment.feedback.as_deref()?)?;
    let file_patterns = review::derive_file_patterns(&comment.file_path);

    Some(FeedbackEvalComment {
        source_kind: source_kind.to_string(),
        review_id,
        repo,
        pr_number,
        title,
        file_path: Some(comment.file_path),
        line_number: Some(comment.line_number),
        file_patterns,
        content: comment.content,
        rule_id: comment.rule_id,
        category: comment.category.to_string(),
        severity: Some(comment.severity.to_string()),
        confidence: Some(comment.confidence),
        accepted,
    })
}
