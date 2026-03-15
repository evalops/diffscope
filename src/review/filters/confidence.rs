use tracing::info;

use crate::core;

use super::super::feedback::{derive_file_patterns, FeedbackPatternStats, FeedbackStore};
use super::super::rule_helpers::normalize_rule_id;

#[derive(Debug, Clone, Copy)]
struct FeedbackConfidenceStats {
    acceptance_rate: f32,
    total: f32,
}

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
    let timestamp = chrono::Utc::now().timestamp();

    comments
        .into_iter()
        .map(|mut comment| {
            if feedback.accept.contains(&comment.id) {
                comment.confidence = (comment.confidence * 1.15).clamp(0.0, 1.0);
                push_feedback_calibration_tag(&mut comment, "feedback-calibration:accepted-id");
            }
            if let Some(stats) = lookup_feedback_confidence_stats(&comment, feedback, timestamp) {
                if stats.total >= min_observations as f32 {
                    let rate = stats.acceptance_rate;
                    let adjustment = 0.75 + rate * 0.5;
                    let previous_confidence = comment.confidence;
                    comment.confidence = (comment.confidence * adjustment).clamp(0.0, 1.0);

                    if comment.confidence > previous_confidence {
                        push_feedback_calibration_tag(&mut comment, "feedback-calibration:boosted");
                    } else if comment.confidence < previous_confidence {
                        push_feedback_calibration_tag(&mut comment, "feedback-calibration:demoted");
                    }
                }
            }

            comment
        })
        .collect()
}

fn push_feedback_calibration_tag(comment: &mut core::Comment, tag: &str) {
    push_feedback_tag(comment, "feedback-calibration");
    push_feedback_tag(comment, tag);
}

fn push_feedback_tag(comment: &mut core::Comment, tag: &str) {
    if !comment.tags.iter().any(|existing| existing == tag) {
        comment.tags.push(tag.to_string());
    }
}

fn lookup_feedback_confidence_stats(
    comment: &core::Comment,
    feedback: &FeedbackStore,
    timestamp: i64,
) -> Option<FeedbackConfidenceStats> {
    let category = comment.category.to_string();
    let file_patterns = derive_file_patterns(&comment.file_path);
    let rule_id = normalize_rule_id(comment.rule_id.as_deref());

    rule_id
        .as_deref()
        .and_then(|rule_id| {
            file_patterns.iter().find_map(|pattern| {
                let key = format!("{rule_id}|{pattern}");
                feedback
                    .by_rule_file_pattern
                    .get(&key)
                    .map(|stats| rule_confidence_stats(stats, timestamp))
            })
        })
        .or_else(|| {
            rule_id.as_deref().and_then(|rule_id| {
                feedback
                    .by_rule
                    .get(rule_id)
                    .map(|stats| rule_confidence_stats(stats, timestamp))
            })
        })
        .or_else(|| {
            file_patterns.iter().find_map(|pattern| {
                let key = format!("{category}|{pattern}");
                feedback
                    .by_category_file_pattern
                    .get(&key)
                    .map(raw_confidence_stats)
            })
        })
        .or_else(|| {
            file_patterns.iter().find_map(|pattern| {
                feedback
                    .by_file_pattern
                    .get(pattern)
                    .map(raw_confidence_stats)
            })
        })
        .or_else(|| {
            feedback
                .by_category
                .get(&category)
                .map(raw_confidence_stats)
        })
}

fn raw_confidence_stats(stats: &FeedbackPatternStats) -> FeedbackConfidenceStats {
    FeedbackConfidenceStats {
        acceptance_rate: stats.weighted_acceptance_rate(),
        total: stats.weighted_total(),
    }
}

fn rule_confidence_stats(stats: &FeedbackPatternStats, timestamp: i64) -> FeedbackConfidenceStats {
    FeedbackConfidenceStats {
        acceptance_rate: stats
            .decayed_acceptance_rate_at(timestamp)
            .unwrap_or_else(|| stats.weighted_acceptance_rate()),
        total: stats
            .decayed_total_at(timestamp)
            .unwrap_or_else(|| stats.weighted_total()),
    }
}
