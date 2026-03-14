use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;

use crate::commands::eval::EvalReport;

use super::super::types::{FeedbackEvalTrend, FeedbackEvalTrendEntry, FeedbackEvalTrendGap};
use super::super::{FeedbackEvalReport, FeedbackEvalRuleCorrelation};

const MAX_ATTENTION_GAPS: usize = 5;

pub(in super::super) async fn update_feedback_eval_trend(
    report: &FeedbackEvalReport,
    eval_report: Option<&EvalReport>,
    path: &Path,
) -> Result<()> {
    let mut trend = if path.exists() {
        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read feedback trend file {}", path.display()))?;
        FeedbackEvalTrend::from_json(&content)
            .with_context(|| format!("failed to parse feedback trend file {}", path.display()))?
    } else {
        FeedbackEvalTrend::new()
    };
    trend
        .entries
        .push(trend_entry_for_report(report, eval_report));

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    tokio::fs::write(path, trend.to_json()?)
        .await
        .with_context(|| format!("failed to write feedback trend file {}", path.display()))?;
    Ok(())
}

fn trend_entry_for_report(
    report: &FeedbackEvalReport,
    eval_report: Option<&EvalReport>,
) -> FeedbackEvalTrendEntry {
    let confidence_metrics = report.confidence_metrics;
    let correlation = report.eval_correlation.as_ref();
    FeedbackEvalTrendEntry {
        timestamp: Utc::now().to_rfc3339(),
        labeled_comments: report.labeled_comments,
        accepted: report.accepted,
        rejected: report.rejected,
        acceptance_rate: report.acceptance_rate,
        confidence_threshold: report.confidence_threshold,
        confidence_agreement_rate: confidence_metrics.map(|metrics| metrics.agreement_rate),
        confidence_precision: confidence_metrics.map(|metrics| metrics.precision),
        confidence_recall: confidence_metrics.map(|metrics| metrics.recall),
        confidence_f1: confidence_metrics.map(|metrics| metrics.f1),
        eval_label: eval_report.and_then(|report| report.run.label.clone()),
        eval_model: eval_report.map(|report| report.run.model.clone()),
        eval_provider: eval_report.and_then(|report| report.run.provider.clone()),
        attention_by_category: correlation
            .map(|report| {
                report
                    .attention_by_category
                    .iter()
                    .take(MAX_ATTENTION_GAPS)
                    .map(|category| FeedbackEvalTrendGap {
                        name: category.name.clone(),
                        feedback_total: category.feedback_total,
                        high_confidence_total: category.high_confidence_total,
                        high_confidence_acceptance_rate: category.high_confidence_acceptance_rate,
                        eval_score: category.eval_micro_f1,
                        gap: category.high_confidence_vs_eval_gap,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        attention_by_rule: correlation
            .map(|report| {
                report
                    .attention_by_rule
                    .iter()
                    .take(MAX_ATTENTION_GAPS)
                    .map(rule_gap)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn rule_gap(rule: &FeedbackEvalRuleCorrelation) -> FeedbackEvalTrendGap {
    FeedbackEvalTrendGap {
        name: rule.rule_id.clone(),
        feedback_total: rule.feedback_total,
        high_confidence_total: rule.high_confidence_total,
        high_confidence_acceptance_rate: rule.high_confidence_acceptance_rate,
        eval_score: rule.eval_f1,
        gap: rule.high_confidence_vs_eval_gap,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::commands::eval::EvalRunMetadata;
    use crate::commands::feedback_eval::{
        FeedbackEvalBucket, FeedbackEvalCategoryCorrelation, FeedbackEvalCorrelationReport,
        FeedbackEvalReport, FeedbackEvalRuleCorrelation, FeedbackThresholdMetrics,
    };

    use super::*;

    fn sample_feedback_report() -> FeedbackEvalReport {
        FeedbackEvalReport {
            total_comments_seen: 12,
            total_reviews_seen: 3,
            labeled_comments: 8,
            labeled_reviews: 2,
            accepted: 3,
            rejected: 5,
            acceptance_rate: 0.375,
            confidence_threshold: 0.75,
            vague_comments: FeedbackEvalBucket {
                name: "vague".to_string(),
                total: 1,
                accepted: 0,
                rejected: 1,
                acceptance_rate: 0.0,
            },
            confidence_metrics: Some(FeedbackThresholdMetrics {
                total_scored: 6,
                true_positive: 2,
                false_positive: 1,
                true_negative: 2,
                false_negative: 1,
                precision: 0.67,
                recall: 0.67,
                f1: 0.67,
                agreement_rate: 0.67,
            }),
            by_category: vec![],
            by_rule: vec![],
            high_confidence_by_category: vec![],
            high_confidence_by_rule: vec![],
            by_severity: vec![],
            by_repo: vec![],
            by_file_pattern: vec![],
            eval_correlation: Some(FeedbackEvalCorrelationReport {
                by_category: vec![],
                by_rule: vec![],
                attention_by_category: vec![FeedbackEvalCategoryCorrelation {
                    name: "Security".to_string(),
                    feedback_total: 4,
                    feedback_acceptance_rate: 0.25,
                    high_confidence_total: 3,
                    high_confidence_acceptance_rate: 0.0,
                    eval_fixture_count: Some(5),
                    eval_micro_f1: Some(0.9),
                    eval_weighted_score: Some(0.91),
                    feedback_vs_eval_gap: Some(0.65),
                    high_confidence_vs_eval_gap: Some(0.9),
                }],
                attention_by_rule: vec![FeedbackEvalRuleCorrelation {
                    rule_id: "sec.sql.injection".to_string(),
                    feedback_total: 3,
                    feedback_acceptance_rate: 0.33,
                    high_confidence_total: 2,
                    high_confidence_acceptance_rate: 0.0,
                    eval_precision: Some(1.0),
                    eval_recall: Some(1.0),
                    eval_f1: Some(1.0),
                    feedback_vs_eval_gap: Some(0.67),
                    high_confidence_vs_eval_gap: Some(1.0),
                }],
            }),
            showcase_candidates: vec![],
            vague_rejections: vec![],
        }
    }

    fn sample_eval_report() -> EvalReport {
        EvalReport {
            run: EvalRunMetadata {
                label: Some("frontier-e2e".to_string()),
                model: "anthropic/claude-opus-4.5".to_string(),
                provider: Some("openrouter".to_string()),
                ..Default::default()
            },
            fixtures_total: 0,
            fixtures_passed: 0,
            fixtures_failed: 0,
            rule_metrics: vec![],
            rule_summary: None,
            benchmark_summary: None,
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            verification_health: None,
            warnings: vec![],
            threshold_failures: vec![],
            results: vec![],
        }
    }

    #[tokio::test]
    async fn update_feedback_eval_trend_appends_attention_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("feedback-trend.json");

        update_feedback_eval_trend(
            &sample_feedback_report(),
            Some(&sample_eval_report()),
            &path,
        )
        .await
        .unwrap();
        update_feedback_eval_trend(
            &sample_feedback_report(),
            Some(&sample_eval_report()),
            &path,
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let trend = FeedbackEvalTrend::from_json(&content).unwrap();
        assert_eq!(trend.entries.len(), 2);
        assert_eq!(trend.entries[0].eval_label.as_deref(), Some("frontier-e2e"));
        assert_eq!(
            trend.entries[0].eval_model.as_deref(),
            Some("anthropic/claude-opus-4.5")
        );
        assert_eq!(trend.entries[0].attention_by_category.len(), 1);
        assert_eq!(trend.entries[0].attention_by_category[0].name, "Security");
        assert_eq!(
            trend.entries[0].attention_by_rule[0].name,
            "sec.sql.injection"
        );
    }
}
