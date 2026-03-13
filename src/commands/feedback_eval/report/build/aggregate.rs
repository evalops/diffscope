#[path = "aggregate/buckets.rs"]
mod buckets;
#[path = "aggregate/correlation.rs"]
mod correlation;
#[path = "aggregate/overview.rs"]
mod overview;

use crate::commands::eval::EvalReport;

use super::super::super::{FeedbackEvalReport, LoadedFeedbackEvalInput};
use super::super::examples::{build_showcase_candidates, build_vague_rejections};
use super::stats::{buckets_from_counts, build_threshold_metrics, ratio};
use buckets::{collect_feedback_bucket_counts, collect_high_confidence_bucket_counts};
use correlation::build_feedback_eval_correlation;
use overview::build_feedback_overview;

#[cfg(test)]
use super::super::super::FeedbackEvalComment;

pub(in super::super::super) fn build_feedback_eval_report(
    loaded: &LoadedFeedbackEvalInput,
    confidence_threshold: f32,
    eval_report: Option<&EvalReport>,
) -> FeedbackEvalReport {
    let overview = build_feedback_overview(loaded);
    let bucket_counts = collect_feedback_bucket_counts(&loaded.comments);
    let high_confidence_bucket_counts =
        collect_high_confidence_bucket_counts(&loaded.comments, confidence_threshold);
    let by_category = buckets_from_counts(bucket_counts.category_counts);
    let by_rule = buckets_from_counts(bucket_counts.rule_counts);
    let high_confidence_by_category =
        buckets_from_counts(high_confidence_bucket_counts.category_counts);
    let high_confidence_by_rule = buckets_from_counts(high_confidence_bucket_counts.rule_counts);

    FeedbackEvalReport {
        total_comments_seen: loaded.total_comments_seen,
        total_reviews_seen: loaded.total_reviews_seen,
        labeled_comments: loaded.comments.len(),
        labeled_reviews: overview.labeled_reviews,
        accepted: overview.accepted,
        rejected: overview.rejected,
        acceptance_rate: ratio(overview.accepted, loaded.comments.len()),
        confidence_threshold,
        vague_comments: overview.vague_bucket,
        confidence_metrics: build_threshold_metrics(&loaded.comments, confidence_threshold),
        by_category: by_category.clone(),
        by_rule: by_rule.clone(),
        high_confidence_by_category: high_confidence_by_category.clone(),
        high_confidence_by_rule: high_confidence_by_rule.clone(),
        by_severity: buckets_from_counts(bucket_counts.severity_counts),
        by_repo: buckets_from_counts(bucket_counts.repo_counts),
        by_file_pattern: buckets_from_counts(bucket_counts.file_pattern_counts),
        eval_correlation: build_feedback_eval_correlation(
            eval_report,
            &by_category,
            &high_confidence_by_category,
            &by_rule,
            &high_confidence_by_rule,
        ),
        showcase_candidates: build_showcase_candidates(&loaded.comments, confidence_threshold),
        vague_rejections: build_vague_rejections(&loaded.comments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn build_feedback_eval_report_tracks_vagueness_and_threshold_metrics() {
        let loaded = LoadedFeedbackEvalInput {
            total_comments_seen: 3,
            total_reviews_seen: 1,
            comments: vec![
                FeedbackEvalComment {
                    source_kind: "review-session".to_string(),
                    review_id: Some("review-1".to_string()),
                    repo: Some("owner/repo".to_string()),
                    pr_number: Some(12),
                    title: Some("Fix query path".to_string()),
                    file_path: Some(PathBuf::from("src/lib.rs")),
                    line_number: Some(10),
                    file_patterns: vec!["*.rs".to_string()],
                    content: "User-controlled SQL is interpolated into the query string."
                        .to_string(),
                    rule_id: Some("sec.sql.injection".to_string()),
                    category: "Security".to_string(),
                    severity: Some("Warning".to_string()),
                    confidence: Some(0.9),
                    accepted: true,
                },
                FeedbackEvalComment {
                    source_kind: "review-session".to_string(),
                    review_id: Some("review-1".to_string()),
                    repo: Some("owner/repo".to_string()),
                    pr_number: Some(12),
                    title: Some("Fix query path".to_string()),
                    file_path: Some(PathBuf::from("src/lib.rs")),
                    line_number: Some(14),
                    file_patterns: vec!["*.rs".to_string()],
                    content: "Consider adding a guard clause for this branch.".to_string(),
                    rule_id: Some("bug.guard-clause".to_string()),
                    category: "Bug".to_string(),
                    severity: Some("Suggestion".to_string()),
                    confidence: Some(0.85),
                    accepted: false,
                },
                FeedbackEvalComment {
                    source_kind: "review-session".to_string(),
                    review_id: Some("review-1".to_string()),
                    repo: Some("owner/repo".to_string()),
                    pr_number: Some(12),
                    title: Some("Fix query path".to_string()),
                    file_path: Some(PathBuf::from("src/lib.rs")),
                    line_number: Some(18),
                    file_patterns: vec!["*.rs".to_string()],
                    content: "Nil dereference is possible when the config lookup fails."
                        .to_string(),
                    rule_id: None,
                    category: "Bug".to_string(),
                    severity: Some("Info".to_string()),
                    confidence: Some(0.4),
                    accepted: true,
                },
            ],
        };

        let report = build_feedback_eval_report(&loaded, 0.8, None);

        assert_eq!(report.labeled_comments, 3);
        assert_eq!(report.accepted, 2);
        assert_eq!(report.rejected, 1);
        assert_eq!(report.vague_comments.total, 1);
        assert_eq!(report.vague_comments.rejected, 1);
        assert_eq!(report.by_rule.len(), 2);
        assert_eq!(report.showcase_candidates.len(), 1);
        assert_eq!(report.vague_rejections.len(), 1);

        let metrics = report.confidence_metrics.unwrap();
        assert_eq!(metrics.true_positive, 1);
        assert_eq!(metrics.false_positive, 1);
        assert_eq!(metrics.false_negative, 1);
        assert!((metrics.precision - 0.5).abs() < f32::EPSILON);
        assert!((metrics.recall - 0.5).abs() < f32::EPSILON);
        assert!((metrics.f1 - 0.5).abs() < f32::EPSILON);
    }
}
