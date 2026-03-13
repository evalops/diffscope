use anyhow::{Context, Result};
use std::path::Path;

use crate::core::eval_benchmarks::{BenchmarkResult, QualityTrend};

use super::super::EvalReport;

pub(in super::super) async fn update_eval_quality_trend(
    report: &EvalReport,
    path: &Path,
) -> Result<()> {
    let Some(result) = benchmark_result_for_report(report) else {
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
    trend.record(&result, report.run.label.as_deref());

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

    use crate::commands::eval::{EvalFixtureResult, EvalReport, EvalRunMetadata};
    use crate::core::eval_benchmarks::{AggregateMetrics, FixtureResult};

    use super::*;

    fn sample_report(label: Option<&str>, timestamp: &str) -> EvalReport {
        EvalReport {
            run: EvalRunMetadata {
                started_at: timestamp.to_string(),
                label: label.map(|value| value.to_string()),
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
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
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
    }
}
