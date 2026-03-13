use super::super::super::super::{FeedbackEvalComment, FeedbackThresholdMetrics};
use super::buckets::ratio;

pub(in super::super) fn build_threshold_metrics(
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

fn harmonic_mean(left: f32, right: f32) -> f32 {
    if left + right <= f32::EPSILON {
        0.0
    } else {
        2.0 * left * right / (left + right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn build_comment(accepted: bool, confidence: Option<f32>) -> FeedbackEvalComment {
        FeedbackEvalComment {
            source_kind: "review-session".to_string(),
            review_id: Some("review-1".to_string()),
            repo: Some("owner/repo".to_string()),
            pr_number: Some(12),
            title: Some("Fix query path".to_string()),
            file_path: Some(PathBuf::from("src/lib.rs")),
            line_number: Some(10),
            file_patterns: vec!["*.rs".to_string()],
            content: "User-controlled SQL is interpolated into the query string.".to_string(),
            rule_id: Some("sec.sql.injection".to_string()),
            category: "Security".to_string(),
            severity: Some("Warning".to_string()),
            confidence,
            accepted,
        }
    }

    #[test]
    fn build_threshold_metrics_scores_confusion_matrix_counts() {
        let comments = vec![
            build_comment(true, Some(0.9)),
            build_comment(false, Some(0.8)),
            build_comment(false, Some(0.2)),
            build_comment(true, Some(0.1)),
        ];

        let metrics = build_threshold_metrics(&comments, 0.5).unwrap();

        assert_eq!(metrics.total_scored, 4);
        assert_eq!(metrics.true_positive, 1);
        assert_eq!(metrics.false_positive, 1);
        assert_eq!(metrics.true_negative, 1);
        assert_eq!(metrics.false_negative, 1);
        assert!((metrics.precision - 0.5).abs() < f32::EPSILON);
        assert!((metrics.recall - 0.5).abs() < f32::EPSILON);
        assert!((metrics.f1 - 0.5).abs() < f32::EPSILON);
        assert!((metrics.agreement_rate - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn build_threshold_metrics_returns_none_without_scored_comments() {
        let comments = vec![build_comment(true, None)];
        assert!(build_threshold_metrics(&comments, 0.5).is_none());
    }
}
