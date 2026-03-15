use anyhow::Result;
use std::path::Path;

use crate::commands::eval::EvalReport;

use super::super::report::{
    build_feedback_eval_report, print_feedback_eval_report, update_feedback_eval_trend,
    write_feedback_eval_report,
};
use super::super::{FeedbackEvalReport, LoadedFeedbackEvalInput};

pub(super) async fn emit_feedback_eval_report(
    loaded: &LoadedFeedbackEvalInput,
    output_path: Option<&Path>,
    trend_path: Option<&Path>,
    trend_max_entries: usize,
    confidence_threshold: f32,
    eval_report: Option<&EvalReport>,
    min_feedback_coverage: Option<f32>,
) -> Result<()> {
    let mut report =
        build_feedback_eval_report(loaded, confidence_threshold.clamp(0.0, 1.0), eval_report);
    report.threshold_failures = evaluate_feedback_eval_thresholds(&report, min_feedback_coverage);
    print_feedback_eval_report(&report);

    if let Some(path) = output_path {
        write_feedback_eval_report(&report, path).await?;
    }
    if let Some(path) = trend_path {
        update_feedback_eval_trend(&report, eval_report, path, trend_max_entries).await?;
    }

    if !report.threshold_failures.is_empty() {
        anyhow::bail!(
            "Feedback eval failed: {}",
            report.threshold_failures.join("; ")
        );
    }

    Ok(())
}

fn evaluate_feedback_eval_thresholds(
    report: &FeedbackEvalReport,
    min_feedback_coverage: Option<f32>,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(threshold) = min_feedback_coverage {
        if report.feedback_coverage_rate < threshold {
            failures.push(format!(
                "feedback coverage {:.3} fell below minimum {:.3} ({}/{})",
                report.feedback_coverage_rate,
                threshold,
                report.labeled_comments,
                report.total_comments_seen
            ));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::feedback_eval::FeedbackEvalBucket;

    fn sample_report() -> FeedbackEvalReport {
        FeedbackEvalReport {
            total_comments_seen: 12,
            total_reviews_seen: 3,
            labeled_comments: 8,
            labeled_reviews: 2,
            accepted: 3,
            rejected: 5,
            acceptance_rate: 0.375,
            feedback_coverage_rate: 8.0 / 12.0,
            confidence_threshold: 0.75,
            vague_comments: FeedbackEvalBucket {
                name: "vague".to_string(),
                total: 1,
                accepted: 0,
                rejected: 1,
                acceptance_rate: 0.0,
            },
            confidence_metrics: None,
            by_category: vec![],
            by_rule: vec![],
            high_confidence_by_category: vec![],
            high_confidence_by_rule: vec![],
            by_severity: vec![],
            by_repo: vec![],
            by_file_pattern: vec![],
            eval_correlation: None,
            showcase_candidates: vec![],
            vague_rejections: vec![],
            threshold_failures: vec![],
        }
    }

    #[test]
    fn evaluate_feedback_eval_thresholds_checks_feedback_coverage() {
        let failures = evaluate_feedback_eval_thresholds(&sample_report(), Some(0.8));

        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("feedback coverage 0.667"));
    }

    #[test]
    fn evaluate_feedback_eval_thresholds_passes_when_coverage_meets_threshold() {
        assert!(evaluate_feedback_eval_thresholds(&sample_report(), Some(0.6)).is_empty());
    }
}
