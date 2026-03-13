use anyhow::Result;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::review;

use super::{
    FeedbackEvalBucket, FeedbackEvalComment, FeedbackEvalExample, FeedbackEvalReport,
    FeedbackThresholdMetrics, LoadedFeedbackEvalInput,
};

pub(super) fn build_feedback_eval_report(
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

    let mut showcase_candidates = loaded
        .comments
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
    showcase_candidates.sort_by(compare_feedback_examples);
    showcase_candidates.truncate(10);

    let mut vague_rejections = loaded
        .comments
        .iter()
        .filter(|comment| !comment.accepted && review::is_vague_comment_text(&comment.content))
        .map(FeedbackEvalExample::from)
        .collect::<Vec<_>>();
    vague_rejections.sort_by(compare_feedback_examples);
    vague_rejections.truncate(10);

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
        showcase_candidates,
        vague_rejections,
    }
}

pub(super) fn print_feedback_eval_report(report: &FeedbackEvalReport) {
    println!(
        "Feedback eval: {}/{} labeled comment(s) across {} review(s)",
        report.labeled_comments, report.total_comments_seen, report.labeled_reviews
    );
    println!(
        "Accepted: {} | Rejected: {} | Acceptance rate: {:.0}%",
        report.accepted,
        report.rejected,
        report.acceptance_rate * 100.0
    );
    println!(
        "Vague comments: {} total | {} accepted | {} rejected | {:.0}% acceptance",
        report.vague_comments.total,
        report.vague_comments.accepted,
        report.vague_comments.rejected,
        report.vague_comments.acceptance_rate * 100.0
    );

    if let Some(metrics) = report.confidence_metrics {
        println!(
            "Confidence@{:.2}: agreement={:.0}% precision={:.0}% recall={:.0}% F1={:.0}% ({})",
            report.confidence_threshold,
            metrics.agreement_rate * 100.0,
            metrics.precision * 100.0,
            metrics.recall * 100.0,
            metrics.f1 * 100.0,
            metrics.total_scored
        );
    }

    for bucket in report.by_category.iter().take(6) {
        println!(
            "Category {}: {}/{} accepted ({:.0}%)",
            bucket.name,
            bucket.accepted,
            bucket.total,
            bucket.acceptance_rate * 100.0
        );
    }

    if !report.showcase_candidates.is_empty() {
        println!(
            "Showcase candidates: {} accepted non-vague comment(s)",
            report.showcase_candidates.len()
        );
    }
    if !report.vague_rejections.is_empty() {
        println!(
            "Vague rejections: {} example(s) reinforce the anti-vague filter",
            report.vague_rejections.len()
        );
    }
}

pub(super) async fn write_feedback_eval_report(
    report: &FeedbackEvalReport,
    path: &Path,
) -> Result<()> {
    let serialized = serde_json::to_string_pretty(report)?;
    tokio::fs::write(path, serialized).await?;
    Ok(())
}

fn build_threshold_metrics(
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

fn compare_feedback_examples(left: &FeedbackEvalExample, right: &FeedbackEvalExample) -> Ordering {
    right
        .confidence
        .partial_cmp(&left.confidence)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            severity_rank(right.severity.as_deref()).cmp(&severity_rank(left.severity.as_deref()))
        })
        .then_with(|| left.content.cmp(&right.content))
}

fn severity_rank(severity: Option<&str>) -> usize {
    match severity.map(|value| value.to_ascii_lowercase()) {
        Some(value) if value == "error" => 3,
        Some(value) if value == "warning" => 2,
        Some(value) if value == "info" => 1,
        _ => 0,
    }
}

fn add_bucket_count(counts: &mut HashMap<String, (usize, usize)>, name: &str, accepted: bool) {
    let entry = counts.entry(name.to_string()).or_default();
    if accepted {
        entry.0 += 1;
    } else {
        entry.1 += 1;
    }
}

fn buckets_from_counts(counts: HashMap<String, (usize, usize)>) -> Vec<FeedbackEvalBucket> {
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

fn build_bucket(name: String, total: usize, accepted: usize) -> FeedbackEvalBucket {
    FeedbackEvalBucket {
        name,
        total,
        accepted,
        rejected: total.saturating_sub(accepted),
        acceptance_rate: ratio(accepted, total),
    }
}

fn ratio(numerator: usize, denominator: usize) -> f32 {
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
