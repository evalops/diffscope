use std::cmp::Ordering;

use crate::review;

use super::super::{FeedbackEvalComment, FeedbackEvalExample};

const MAX_EXAMPLES: usize = 10;

pub(super) fn build_showcase_candidates(
    comments: &[FeedbackEvalComment],
    confidence_threshold: f32,
) -> Vec<FeedbackEvalExample> {
    let mut examples = comments
        .iter()
        .filter(|comment| {
            comment.accepted
                && !review::is_vague_comment_text(&comment.content)
                && comment
                    .confidence
                    .map(|confidence| confidence >= confidence_threshold)
                    .unwrap_or(true)
        })
        .map(FeedbackEvalExample::from)
        .collect::<Vec<_>>();
    examples.sort_by(compare_feedback_examples);
    examples.truncate(MAX_EXAMPLES);
    examples
}

pub(super) fn build_vague_rejections(comments: &[FeedbackEvalComment]) -> Vec<FeedbackEvalExample> {
    let mut examples = comments
        .iter()
        .filter(|comment| !comment.accepted && review::is_vague_comment_text(&comment.content))
        .map(FeedbackEvalExample::from)
        .collect::<Vec<_>>();
    examples.sort_by(compare_feedback_examples);
    examples.truncate(MAX_EXAMPLES);
    examples
}

fn compare_feedback_examples(left: &FeedbackEvalExample, right: &FeedbackEvalExample) -> Ordering {
    right
        .confidence
        .partial_cmp(&left.confidence)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            severity_rank(right.severity.as_deref()).cmp(&severity_rank(left.severity.as_deref()))
        })
        .then_with(|| left.content.cmp(&right.content))
}

fn severity_rank(severity: Option<&str>) -> usize {
    match severity.map(|value| value.to_ascii_lowercase()) {
        Some(value) if value == "error" => 3,
        Some(value) if value == "warning" => 2,
        Some(value) if value == "info" => 1,
        _ => 0,
    }
}

impl From<&FeedbackEvalComment> for FeedbackEvalExample {
    fn from(comment: &FeedbackEvalComment) -> Self {
        Self {
            source_kind: comment.source_kind.clone(),
            review_id: comment.review_id.clone(),
            repo: comment.repo.clone(),
            pr_number: comment.pr_number,
            title: comment.title.clone(),
            file_path: comment.file_path.clone(),
            line_number: comment.line_number,
            category: comment.category.clone(),
            severity: comment.severity.clone(),
            confidence: comment.confidence,
            content: comment.content.clone(),
        }
    }
}
