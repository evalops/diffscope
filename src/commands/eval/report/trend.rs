use anyhow::{Context, Result};
use std::path::Path;

use crate::core::eval_benchmarks::{BenchmarkResult, QualityTrend, TrendEntry};

use super::super::EvalReport;

pub(in super::super) async fn update_eval_quality_trend(
    report: &EvalReport,
    path: &Path,
) -> Result<()> {
    let Some(entry) = trend_entry_for_report(report) else {
        return Ok(());
    };

    let mut trend = if path.exists() {
        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read trend file {}", path.display()))?;
        QualityTrend::from_json(&content)
            .with_context(|| format!("failed to parse trend file {}", path.display()))?
    } else {
        QualityTrend::new()
    };
    trend.entries.push(entry);

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    tokio::fs::write(path, trend.to_json()?)
        .await
        .with_context(|| format!("failed to write trend file {}", path.display()))?;
    Ok(())
}

fn trend_entry_for_report(report: &EvalReport) -> Option<TrendEntry> {
    let result = benchmark_result_for_report(report)?;
    let verification_health = report.verification_health.as_ref();

    Some(TrendEntry {
        timestamp: result.timestamp.clone(),
        micro_f1: result.aggregate.micro_f1,
        micro_precision: result.aggregate.micro_precision,
        micro_recall: result.aggregate.micro_recall,
        fixture_count: result.fixture_results.len(),
        label: report.run.label.clone(),
        weighted_score: Some(result.aggregate.weighted_score),
        model: (!report.run.model.is_empty()).then(|| report.run.model.clone()),
        provider: report.run.provider.clone(),
        suite_micro_f1: report
            .suite_results
            .iter()
            .map(|suite| (suite.suite.clone(), suite.aggregate.micro_f1))
            .collect(),
        category_micro_f1: report
            .benchmark_by_category
            .iter()
            .map(|(name, metrics)| (name.clone(), metrics.micro_f1))
            .collect(),
        language_micro_f1: report
            .benchmark_by_language
            .iter()
            .map(|(name, metrics)| (name.clone(), metrics.micro_f1))
            .collect(),
        verification_warning_count: verification_health.map(|health| health.warnings_total),
        verification_fail_open_count: verification_health
            .map(|health| health.fail_open_warning_count),
        verification_parse_failure_count: verification_health
            .map(|health| health.parse_failure_count),
        verification_request_failure_count: verification_health
            .map(|health| health.request_failure_count),
    })
}

fn benchmark_result_for_report(report: &EvalReport) -> Option<BenchmarkResult> {
    let aggregate = report.benchmark_summary.clone()?;
    let fixture_results = report
        .results
        .iter()
        .filter_map(|result| result.benchmark_metrics.clone())
        .collect::<Vec<_>>();
    if fixture_results.is_empty() {
        return None;
    }

    Some(BenchmarkResult {
        suite_name: report
            .run
            .label
            .clone()
            .unwrap_or_else(|| "eval".to_string()),
        fixture_results,
        aggregate,
        by_category: report.benchmark_by_category.clone(),
        by_difficulty: report.benchmark_by_difficulty.clone(),
        threshold_pass: report.threshold_failures.is_empty(),
        threshold_failures: report.threshold_failures.clone(),
        timestamp: report.run.started_at.clone(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::commands::eval::{
        EvalFixtureResult, EvalReport, EvalRunMetadata, EvalSuiteResult, EvalVerificationHealth,
    };
    use crate::core::eval_benchmarks::{AggregateMetrics, FixtureResult};

    use super::*;

    fn sample_report(label: Option<&str>, timestamp: &str) -> EvalReport {
        EvalReport {
            run: EvalRunMetadata {
                started_at: timestamp.to_string(),
                label: label.map(|value| value.to_string()),
                model: "anthropic/claude-opus-4.1".to_string(),
                provider: Some("openrouter".to_string()),
                ..Default::default()
            },
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![],
            rule_summary: None,
            benchmark_summary: Some(AggregateMetrics {
                fixture_count: 1,
                micro_precision: 1.0,
                micro_recall: 1.0,
                micro_f1: 1.0,
                weighted_score: 1.0,
                ..Default::default()
            }),
            suite_results: vec![EvalSuiteResult {
                suite: "deep-review".to_string(),
                fixture_count: 1,
                aggregate: AggregateMetrics {
                    fixture_count: 1,
                    micro_f1: 1.0,
                    weighted_score: 1.0,
                    ..Default::default()
                },
                thresholds_enforced: false,
                threshold_pass: true,
                threshold_failures: vec![],
            }],
            benchmark_by_category: std::collections::HashMap::from([(
                "security".to_string(),
                AggregateMetrics {
                    fixture_count: 1,
                    micro_f1: 1.0,
                    weighted_score: 1.0,
                    ..Default::default()
                },
            )]),
            benchmark_by_language: std::collections::HashMap::from([(
                "rust".to_string(),
                AggregateMetrics {
                    fixture_count: 1,
                    micro_f1: 1.0,
                    weighted_score: 1.0,
                    ..Default::default()
                },
            )]),
            benchmark_by_difficulty: Default::default(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            verification_health: Some(EvalVerificationHealth {
                warnings_total: 2,
                fixtures_with_warnings: 1,
                fail_open_warning_count: 2,
                parse_failure_count: 1,
                request_failure_count: 1,
            }),
            warnings: vec![],
            threshold_failures: vec![],
            results: vec![EvalFixtureResult {
                fixture: "suite/sample".to_string(),
                suite: Some("suite".to_string()),
                passed: true,
                total_comments: 1,
                required_matches: 1,
                required_total: 1,
                benchmark_metrics: Some(FixtureResult::compute("suite/sample", 1, 0, 1, 0, 0)),
                suite_thresholds: None,
                difficulty: None,
                metadata: None,
                rule_metrics: vec![],
                rule_summary: None,
                warnings: vec![],
                verification_report: None,
                agent_activity: None,
                reproduction_summary: None,
                artifact_path: None,
                failures: vec![],
            }],
        }
    }

    #[tokio::test]
    async fn update_eval_quality_trend_appends_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("trend.json");

        update_eval_quality_trend(&sample_report(Some("first"), "2026-03-13T00:00:00Z"), &path)
            .await
            .unwrap();
        update_eval_quality_trend(
            &sample_report(Some("second"), "2026-03-13T00:10:00Z"),
            &path,
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let trend = QualityTrend::from_json(&content).unwrap();
        assert_eq!(trend.entries.len(), 2);
        assert_eq!(trend.entries[0].label.as_deref(), Some("first"));
        assert_eq!(trend.entries[1].label.as_deref(), Some("second"));
        assert_eq!(trend.entries[0].provider.as_deref(), Some("openrouter"));
        assert_eq!(
            trend.entries[0].suite_micro_f1.get("deep-review").copied(),
            Some(1.0)
        );
        assert_eq!(
            trend.entries[0]
                .verification_parse_failure_count
                .unwrap_or_default(),
            1
        );
    }
}
