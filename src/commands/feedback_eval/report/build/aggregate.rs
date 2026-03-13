use std::collections::{HashMap, HashSet};

use crate::review;

use super::super::super::{FeedbackEvalComment, FeedbackEvalReport, LoadedFeedbackEvalInput};
use super::super::examples::{build_showcase_candidates, build_vague_rejections};
use super::stats::{
    add_bucket_count, buckets_from_counts, build_bucket, build_threshold_metrics, ratio,
};

pub(in super::super::super) fn build_feedback_eval_report(
    loaded: &LoadedFeedbackEvalInput,
    confidence_threshold: f32,
) -> FeedbackEvalReport {
    let accepted = loaded
        .comments
        .iter()
        .filter(|comment| comment.accepted)
        .count();
    let rejected = loaded.comments.len().saturating_sub(accepted);
    let labeled_reviews = loaded
        .comments
        .iter()
        .filter_map(|comment| comment.review_id.as_ref())
        .collect::<HashSet<_>>()
        .len();

    let vague_comments: Vec<&FeedbackEvalComment> = loaded
        .comments
        .iter()
        .filter(|comment| review::is_vague_comment_text(&comment.content))
        .collect();
    let vague_accepted = vague_comments
        .iter()
        .filter(|comment| comment.accepted)
        .count();
    let vague_bucket = build_bucket("vague".to_string(), vague_comments.len(), vague_accepted);

    let mut category_counts = HashMap::new();
    let mut severity_counts = HashMap::new();
    let mut repo_counts = HashMap::new();
    let mut file_pattern_counts = HashMap::new();

    for comment in &loaded.comments {
        add_bucket_count(&mut category_counts, &comment.category, comment.accepted);

        let severity = comment.severity.as_deref().unwrap_or("unknown");
        add_bucket_count(&mut severity_counts, severity, comment.accepted);

        if let Some(repo) = comment.repo.as_deref() {
            add_bucket_count(&mut repo_counts, repo, comment.accepted);
        }

        let unique_patterns = comment
            .file_patterns
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for pattern in unique_patterns {
            add_bucket_count(&mut file_pattern_counts, pattern, comment.accepted);
        }
    }

    FeedbackEvalReport {
        total_comments_seen: loaded.total_comments_seen,
        total_reviews_seen: loaded.total_reviews_seen,
        labeled_comments: loaded.comments.len(),
        labeled_reviews,
        accepted,
        rejected,
        acceptance_rate: ratio(accepted, loaded.comments.len()),
        confidence_threshold,
        vague_comments: vague_bucket,
        confidence_metrics: build_threshold_metrics(&loaded.comments, confidence_threshold),
        by_category: buckets_from_counts(category_counts),
        by_severity: buckets_from_counts(severity_counts),
        by_repo: buckets_from_counts(repo_counts),
        by_file_pattern: buckets_from_counts(file_pattern_counts),
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
                    category: "Bug".to_string(),
                    severity: Some("Info".to_string()),
                    confidence: Some(0.4),
                    accepted: true,
                },
            ],
        };

        let report = build_feedback_eval_report(&loaded, 0.8);

        assert_eq!(report.labeled_comments, 3);
        assert_eq!(report.accepted, 2);
        assert_eq!(report.rejected, 1);
        assert_eq!(report.vague_comments.total, 1);
        assert_eq!(report.vague_comments.rejected, 1);
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
