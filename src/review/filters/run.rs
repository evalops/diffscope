use crate::config;
use crate::core;

use super::super::feedback::FeedbackStore;
use super::comment_types::apply_comment_type_filter;
use super::confidence::apply_confidence_threshold;
use super::suppression::apply_feedback_suppression_with_thresholds;
use super::vague::apply_vague_comment_filter;

pub fn apply_review_filters(
    comments: Vec<core::Comment>,
    config: &config::Config,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    let comments = apply_confidence_threshold(comments, config.effective_min_confidence());
    let comments = apply_comment_type_filter(comments, &config.comment_types);
    let comments = apply_vague_comment_filter(comments);
    apply_feedback_suppression_with_thresholds(
        comments,
        feedback,
        config.feedback_suppression_threshold,
        config.feedback_suppression_margin,
    )
}
