use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[path = "feedback_eval/input.rs"]
mod input;
#[path = "feedback_eval/report.rs"]
mod report;

use input::load_feedback_eval_input;
use report::build_feedback_eval_report;

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
