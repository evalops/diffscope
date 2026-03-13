use crate::config;
use crate::core;

use super::lookup::SemanticFeedbackObservations;

pub(super) fn adjust_comment_from_feedback(
    comment: &mut core::Comment,
    observations: SemanticFeedbackObservations,
    config: &config::Config,
) {
    if observations.total() < config.semantic_feedback_min_examples {
        return;
    }

    if observations.rejected > observations.accepted {
        let delta = ((observations.rejected - observations.accepted) as f32 * 0.15).min(0.45);
        comment.confidence = (comment.confidence - delta).clamp(0.0, 1.0);
        push_feedback_tag(comment, "semantic-feedback:rejected");
    } else if observations.accepted > observations.rejected {
        let delta = ((observations.accepted - observations.rejected) as f32 * 0.10).min(0.25);
        comment.confidence = (comment.confidence + delta).clamp(0.0, 1.0);
        push_feedback_tag(comment, "semantic-feedback:accepted");
    }
}

fn push_feedback_tag(comment: &mut core::Comment, tag: &str) {
    if !comment.tags.iter().any(|existing| existing == tag) {
        comment.tags.push(tag.to_string());
    }
}
