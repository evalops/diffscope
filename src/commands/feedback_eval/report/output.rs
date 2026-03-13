use anyhow::Result;
use std::path::Path;

use super::super::FeedbackEvalReport;

pub(in super::super) fn print_feedback_eval_report(report: &FeedbackEvalReport) {
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

pub(in super::super) async fn write_feedback_eval_report(
    report: &FeedbackEvalReport,
    path: &Path,
) -> Result<()> {
    let serialized = serde_json::to_string_pretty(report)?;
    tokio::fs::write(path, serialized).await?;
    Ok(())
}
