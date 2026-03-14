use tracing::info;

use crate::core;

use super::super::feedback::{derive_file_patterns, FeedbackStore};

pub fn apply_confidence_threshold(
    comments: Vec<core::Comment>,
    min_confidence: f32,
) -> Vec<core::Comment> {
    if min_confidence <= 0.0 {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        if comment.confidence >= min_confidence {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) below confidence threshold {}",
            dropped, min_confidence
        );
    }

    kept
}

pub fn apply_feedback_confidence_adjustment(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
    min_observations: usize,
) -> Vec<core::Comment> {
    comments
        .into_iter()
        .map(|mut comment| {
            if let Some(stats) = lookup_feedback_confidence_stats(&comment, feedback) {
                if stats.total() >= min_observations {
                    let rate = stats.acceptance_rate();
                    let adjustment = 0.5 + rate * 0.5;
                    comment.confidence = (comment.confidence * adjustment).clamp(0.0, 1.0);
                }
            }

            comment
        })
        .collect()
}

fn lookup_feedback_confidence_stats<'a>(
    comment: &core::Comment,
    feedback: &'a FeedbackStore,
) -> Option<&'a super::super::feedback::FeedbackPatternStats> {
    let category = comment.category.to_string();
    let file_patterns = derive_file_patterns(&comment.file_path);

    file_patterns
        .iter()
        .find_map(|pattern| {
            let key = format!("{category}|{pattern}");
            feedback.by_category_file_pattern.get(&key)
        })
        .or_else(|| {
            file_patterns
                .iter()
                .find_map(|pattern| feedback.by_file_pattern.get(pattern))
        })
        .or_else(|| feedback.by_category.get(&category))
}
