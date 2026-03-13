use crate::review;

use super::super::super::{FeedbackEvalComment, FeedbackEvalExample};
use super::ranking::rank_feedback_examples;

pub(in super::super) fn build_showcase_candidates(
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
    rank_feedback_examples(&mut examples);
    examples
}

pub(in super::super) fn build_vague_rejections(
    comments: &[FeedbackEvalComment],
) -> Vec<FeedbackEvalExample> {
    let mut examples = comments
        .iter()
        .filter(|comment| !comment.accepted && review::is_vague_comment_text(&comment.content))
        .map(FeedbackEvalExample::from)
        .collect::<Vec<_>>();
    rank_feedback_examples(&mut examples);
    examples
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
