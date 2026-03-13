use std::collections::HashMap;

use super::super::super::{FeedbackEvalBucket, FeedbackEvalComment, FeedbackThresholdMetrics};

pub(super) fn build_threshold_metrics(
    comments: &[FeedbackEvalComment],
    confidence_threshold: f32,
) -> Option<FeedbackThresholdMetrics> {
    let scored_comments = comments
        .iter()
        .filter_map(|comment| comment.confidence.map(|confidence| (comment, confidence)))
        .collect::<Vec<_>>();
    if scored_comments.is_empty() {
        return None;
    }

    let mut metrics = FeedbackThresholdMetrics {
        total_scored: scored_comments.len(),
        ..Default::default()
    };

    for (comment, confidence) in scored_comments {
        let predicted_accepted = confidence >= confidence_threshold;
        match (predicted_accepted, comment.accepted) {
            (true, true) => metrics.true_positive += 1,
            (true, false) => metrics.false_positive += 1,
            (false, false) => metrics.true_negative += 1,
            (false, true) => metrics.false_negative += 1,
        }
    }

    metrics.precision = ratio(
        metrics.true_positive,
        metrics.true_positive + metrics.false_positive,
    );
    metrics.recall = ratio(
        metrics.true_positive,
        metrics.true_positive + metrics.false_negative,
    );
    metrics.f1 = harmonic_mean(metrics.precision, metrics.recall);
    metrics.agreement_rate = ratio(
        metrics.true_positive + metrics.true_negative,
        metrics.total_scored,
    );
    Some(metrics)
}

pub(super) fn add_bucket_count(
    counts: &mut HashMap<String, (usize, usize)>,
    name: &str,
    accepted: bool,
) {
    let entry = counts.entry(name.to_string()).or_default();
    if accepted {
        entry.0 += 1;
    } else {
        entry.1 += 1;
    }
}

pub(super) fn buckets_from_counts(
    counts: HashMap<String, (usize, usize)>,
) -> Vec<FeedbackEvalBucket> {
    let mut buckets = counts
        .into_iter()
        .map(|(name, (accepted, rejected))| build_bucket(name, accepted + rejected, accepted))
        .collect::<Vec<_>>();
    buckets.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.name.cmp(&right.name))
    });
    buckets
}

pub(super) fn build_bucket(name: String, total: usize, accepted: usize) -> FeedbackEvalBucket {
    FeedbackEvalBucket {
        name,
        total,
        accepted,
        rejected: total.saturating_sub(accepted),
        acceptance_rate: ratio(accepted, total),
    }
}

pub(super) fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

fn harmonic_mean(left: f32, right: f32) -> f32 {
    if left + right <= f32::EPSILON {
        0.0
    } else {
        2.0 * left * right / (left + right)
    }
}
