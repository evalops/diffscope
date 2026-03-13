use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::core;
use crate::review;
use crate::server::state::ReviewSession;

#[derive(Debug, Clone, Default)]
struct LoadedFeedbackEvalInput {
    total_comments_seen: usize,
    total_reviews_seen: usize,
    comments: Vec<FeedbackEvalComment>,
}

#[derive(Debug, Clone)]
struct FeedbackEvalComment {
    source_kind: String,
    review_id: Option<String>,
    repo: Option<String>,
    pr_number: Option<u32>,
    title: Option<String>,
    file_path: Option<PathBuf>,
    line_number: Option<usize>,
    file_patterns: Vec<String>,
    content: String,
    category: String,
    severity: Option<String>,
    confidence: Option<f32>,
    accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedbackEvalBucket {
    #[serde(default)]
    name: String,
    #[serde(default)]
    total: usize,
    #[serde(default)]
    accepted: usize,
    #[serde(default)]
    rejected: usize,
    #[serde(default)]
    acceptance_rate: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct FeedbackThresholdMetrics {
    #[serde(default)]
    total_scored: usize,
    #[serde(default)]
    true_positive: usize,
    #[serde(default)]
    false_positive: usize,
    #[serde(default)]
    true_negative: usize,
    #[serde(default)]
    false_negative: usize,
    #[serde(default)]
    precision: f32,
    #[serde(default)]
    recall: f32,
    #[serde(default)]
    f1: f32,
    #[serde(default)]
    agreement_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedbackEvalExample {
    #[serde(default)]
    source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pr_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_number: Option<usize>,
    #[serde(default)]
    category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedbackEvalReport {
    #[serde(default)]
    total_comments_seen: usize,
    #[serde(default)]
    total_reviews_seen: usize,
    #[serde(default)]
    labeled_comments: usize,
    #[serde(default)]
    labeled_reviews: usize,
    #[serde(default)]
    accepted: usize,
    #[serde(default)]
    rejected: usize,
    #[serde(default)]
    acceptance_rate: f32,
    #[serde(default)]
    confidence_threshold: f32,
    vague_comments: FeedbackEvalBucket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confidence_metrics: Option<FeedbackThresholdMetrics>,
    #[serde(default)]
    by_category: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    by_severity: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    by_repo: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    by_file_pattern: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    showcase_candidates: Vec<FeedbackEvalExample>,
    #[serde(default)]
    vague_rejections: Vec<FeedbackEvalExample>,
}

pub async fn feedback_eval_command(
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    confidence_threshold: f32,
) -> Result<()> {
    let loaded = load_feedback_eval_input(&input_path).await?;
    if loaded.comments.is_empty() {
        anyhow::bail!(
            "No accepted/rejected feedback examples found in {}",
            input_path.display()
        );
    }

    let report = build_feedback_eval_report(&loaded, confidence_threshold.clamp(0.0, 1.0));

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

    if let Some(path) = output_path {
        let serialized = serde_json::to_string_pretty(&report)?;
        tokio::fs::write(path, serialized).await?;
    }

    Ok(())
}

async fn load_feedback_eval_input(path: &Path) -> Result<LoadedFeedbackEvalInput> {
    let content = tokio::fs::read_to_string(path).await?;
    load_feedback_eval_input_from_str(&content)
}

fn load_feedback_eval_input_from_str(content: &str) -> Result<LoadedFeedbackEvalInput> {
    if let Ok(review_map) = serde_json::from_str::<HashMap<String, ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for (review_id, session) in review_map {
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(review_list) = serde_json::from_str::<Vec<ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for session in review_list {
            let review_id = session.id.clone();
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(store) = serde_json::from_str::<core::SemanticFeedbackStore>(content) {
        let total_comments_seen = store.examples.len();
        let comments = store
            .examples
            .into_iter()
            .map(|example| FeedbackEvalComment {
                source_kind: "semantic-feedback".to_string(),
                review_id: None,
                repo: None,
                pr_number: None,
                title: None,
                file_path: None,
                line_number: None,
                file_patterns: example.file_patterns,
                content: example.content,
                category: example.category,
                severity: None,
                confidence: None,
                accepted: example.accepted,
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    if let Ok(comments) = serde_json::from_str::<Vec<core::Comment>>(content) {
        let total_comments_seen = comments.len();
        let comments = comments
            .into_iter()
            .filter_map(|comment| {
                feedback_comment_from_comment("comments-json", None, None, None, None, comment)
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    anyhow::bail!(
        "Unsupported feedback eval input format: expected reviews.json, a comments array, or semantic feedback store JSON"
    )
}

fn extend_from_review_session(
    loaded: &mut LoadedFeedbackEvalInput,
    review_id: Option<String>,
    session: ReviewSession,
) {
    let repo = session
        .event
        .as_ref()
        .and_then(|event| event.github_repo.clone());
    let pr_number = session.event.as_ref().and_then(|event| event.github_pr);
    let title = session.event.as_ref().and_then(|event| event.title.clone());

    loaded.total_reviews_seen += 1;
    loaded.total_comments_seen += session.comments.len();
    loaded
        .comments
        .extend(session.comments.into_iter().filter_map(|comment| {
            feedback_comment_from_comment(
                "review-session",
                review_id.clone(),
                repo.clone(),
                pr_number,
                title.clone(),
                comment,
            )
        }));
}

fn feedback_comment_from_comment(
    source_kind: &str,
    review_id: Option<String>,
    repo: Option<String>,
    pr_number: Option<u32>,
    title: Option<String>,
    comment: core::Comment,
) -> Option<FeedbackEvalComment> {
    let accepted = normalize_feedback_label(comment.feedback.as_deref()?)?;
    let file_patterns = review::derive_file_patterns(&comment.file_path);

    Some(FeedbackEvalComment {
        source_kind: source_kind.to_string(),
        review_id,
        repo,
        pr_number,
        title,
        file_path: Some(comment.file_path),
        line_number: Some(comment.line_number),
        file_patterns,
        content: comment.content,
        category: comment.category.to_string(),
        severity: Some(comment.severity.to_string()),
        confidence: Some(comment.confidence),
        accepted,
    })
}

fn normalize_feedback_label(label: &str) -> Option<bool> {
    match label.trim().to_ascii_lowercase().as_str() {
        "accept" | "accepted" => Some(true),
        "reject" | "rejected" => Some(false),
        _ => None,
    }
}

fn build_feedback_eval_report(
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

fn compare_feedback_examples(
    left: &FeedbackEvalExample,
    right: &FeedbackEvalExample,
) -> std::cmp::Ordering {
    right
        .confidence
        .partial_cmp(&left.confidence)
        .unwrap_or(std::cmp::Ordering::Equal)
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
    use crate::core::comment::{Category, FixEffort, ReviewSummary, Severity};
    use crate::server::state::ReviewStatus;

    fn make_comment(
        content: &str,
        feedback: Option<&str>,
        confidence: f32,
        category: Category,
        severity: Severity,
    ) -> core::Comment {
        core::Comment {
            id: format!("{}-{}", content, confidence),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: content.to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: feedback.map(str::to_string),
        }
    }

    fn make_review_session(comments: Vec<core::Comment>) -> ReviewSession {
        ReviewSession {
            id: "review-1".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "raw".to_string(),
            started_at: 1,
            completed_at: Some(2),
            comments,
            summary: None::<ReviewSummary>,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn load_feedback_eval_input_supports_review_session_maps() {
        let session = make_review_session(vec![
            make_comment(
                "Concrete issue",
                Some("accept"),
                0.9,
                Category::Security,
                Severity::Warning,
            ),
            make_comment("Unlabeled issue", None, 0.4, Category::Bug, Severity::Info),
        ]);
        let json =
            serde_json::to_string(&HashMap::from([("review-1".to_string(), session)])).unwrap();

        let loaded = load_feedback_eval_input_from_str(&json).unwrap();

        assert_eq!(loaded.total_reviews_seen, 1);
        assert_eq!(loaded.total_comments_seen, 2);
        assert_eq!(loaded.comments.len(), 1);
        assert_eq!(loaded.comments[0].review_id.as_deref(), Some("review-1"));
        assert!(loaded.comments[0].accepted);
    }

    #[test]
    fn load_feedback_eval_input_supports_semantic_feedback_store() {
        let json = serde_json::to_string(&core::SemanticFeedbackStore {
            version: 1,
            examples: vec![core::SemanticFeedbackExample {
                content: "Consider adding a null check".to_string(),
                category: "Bug".to_string(),
                file_patterns: vec!["*.rs".to_string()],
                accepted: false,
                created_at: "2026-03-13T00:00:00Z".to_string(),
                embedding: vec![],
            }],
            embedding: Default::default(),
        })
        .unwrap();

        let loaded = load_feedback_eval_input_from_str(&json).unwrap();

        assert_eq!(loaded.total_comments_seen, 1);
        assert_eq!(loaded.comments.len(), 1);
        assert_eq!(loaded.comments[0].source_kind, "semantic-feedback");
        assert!(!loaded.comments[0].accepted);
    }

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
