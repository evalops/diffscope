use tracing::info;

use crate::core;

use super::super::feedback::FeedbackStore;
use super::comment_types::classify_comment_type;

pub fn should_adaptively_suppress_with_thresholds(
    comment: &core::Comment,
    feedback: &FeedbackStore,
    rejected_threshold: usize,
    margin: usize,
) -> bool {
    if matches!(
        comment.severity,
        core::comment::Severity::Error | core::comment::Severity::Warning
    ) {
        return false;
    }

    let key = classify_comment_type(comment).as_str();
    let stats = match feedback.by_comment_type.get(key) {
        Some(stats) => stats,
        None => return false,
    };

    stats.negative_total() >= rejected_threshold
        && stats.negative_total() >= stats.positive_total().saturating_add(margin)
}

pub fn apply_feedback_suppression_with_thresholds(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
    rejected_threshold: usize,
    margin: usize,
) -> Vec<core::Comment> {
    if feedback.suppress.is_empty() && feedback.by_comment_type.is_empty() {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);
    let mut explicit_dropped = 0usize;
    let mut adaptive_dropped = 0usize;

    for comment in comments {
        if feedback.suppress.contains(&comment.id) {
            explicit_dropped += 1;
            continue;
        }
        if should_adaptively_suppress_with_thresholds(
            &comment,
            feedback,
            rejected_threshold,
            margin,
        ) {
            adaptive_dropped += 1;
            continue;
        }
        kept.push(comment);
    }

    if explicit_dropped > 0 {
        info!(
            "Dropped {} comment(s) due to explicit feedback suppression rules",
            explicit_dropped
        );
    }
    if adaptive_dropped > 0 {
        info!(
            "Dropped {} low-priority comment(s) due to learned feedback preferences",
            adaptive_dropped
        );
    }

    kept
}
