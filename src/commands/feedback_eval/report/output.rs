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
        "Feedback coverage: {}/{} ({:.0}%)",
        report.labeled_comments,
        report.total_comments_seen,
        report.feedback_coverage_rate * 100.0
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

    for bucket in report.by_rule.iter().take(6) {
        println!(
            "Rule {}: {}/{} accepted ({:.0}%)",
            bucket.name,
            bucket.accepted,
            bucket.total,
            bucket.acceptance_rate * 100.0
        );
    }

    for bucket in report.high_confidence_by_category.iter().take(4) {
        println!(
            "High-confidence category {}: {}/{} accepted ({:.0}%)",
            bucket.name,
            bucket.accepted,
            bucket.total,
            bucket.acceptance_rate * 100.0
        );
    }

    for bucket in report.high_confidence_by_rule.iter().take(4) {
        println!(
            "High-confidence rule {}: {}/{} accepted ({:.0}%)",
            bucket.name,
            bucket.accepted,
            bucket.total,
            bucket.acceptance_rate * 100.0
        );
    }

    if let Some(correlation) = report.eval_correlation.as_ref() {
        for bucket in correlation.by_category.iter().take(4) {
            if let Some(eval_micro_f1) = bucket.eval_micro_f1 {
                println!(
                    "Category alignment {}: feedback={:.0}% high-confidence={:.0}% eval-F1={:.0}%",
                    bucket.name,
                    bucket.feedback_acceptance_rate * 100.0,
                    bucket.high_confidence_acceptance_rate * 100.0,
                    eval_micro_f1 * 100.0
                );
            }
        }
        for bucket in correlation.attention_by_category.iter().take(4) {
            if let Some(gap) = bucket.high_confidence_vs_eval_gap {
                println!(
                    "Category attention {}: high-confidence={:.0}% eval-F1={:.0}% gap={:.0}pt ({})",
                    bucket.name,
                    bucket.high_confidence_acceptance_rate * 100.0,
                    bucket.eval_micro_f1.unwrap_or_default() * 100.0,
                    gap * 100.0,
                    bucket.high_confidence_total
                );
            }
        }
        for bucket in correlation.by_rule.iter().take(4) {
            if let Some(eval_f1) = bucket.eval_f1 {
                println!(
                    "Rule alignment {}: feedback={:.0}% high-confidence={:.0}% eval-F1={:.0}%",
                    bucket.rule_id,
                    bucket.feedback_acceptance_rate * 100.0,
                    bucket.high_confidence_acceptance_rate * 100.0,
                    eval_f1 * 100.0
                );
            }
        }
        for bucket in correlation.attention_by_rule.iter().take(4) {
            if let Some(gap) = bucket.high_confidence_vs_eval_gap {
                println!(
                    "Rule attention {}: high-confidence={:.0}% eval-F1={:.0}% gap={:.0}pt ({})",
                    bucket.rule_id,
                    bucket.high_confidence_acceptance_rate * 100.0,
                    bucket.eval_f1.unwrap_or_default() * 100.0,
                    gap * 100.0,
                    bucket.high_confidence_total
                );
            }
        }
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

    for failure in &report.threshold_failures {
        println!("Threshold failure: {failure}");
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
