use std::collections::HashMap;

use crate::core::eval_benchmarks::AggregateMetrics as BenchmarkAggregateMetrics;

use super::super::{
    EvalFixtureResult, EvalNamedMetricComparison, EvalReport, EvalSuiteResult,
    EvalVerificationHealth,
};

pub(in super::super) fn build_suite_comparisons(
    current: &[EvalSuiteResult],
    baseline: Option<&EvalReport>,
) -> Vec<EvalNamedMetricComparison> {
    let Some(baseline) = baseline else {
        return Vec::new();
    };

    let baseline_by_suite = baseline
        .suite_results
        .iter()
        .map(|suite| (suite.suite.as_str(), &suite.aggregate))
        .collect::<HashMap<_, _>>();

    let mut comparisons = current
        .iter()
        .filter_map(|suite| {
            let baseline_metrics = baseline_by_suite.get(suite.suite.as_str())?;
            Some(build_comparison(
                suite.suite.clone(),
                &suite.aggregate,
                baseline_metrics,
            ))
        })
        .collect::<Vec<_>>();
    comparisons.sort_by(|left, right| left.name.cmp(&right.name));
    comparisons
}

pub(in super::super) fn build_named_breakdown_comparisons(
    current: &HashMap<String, BenchmarkAggregateMetrics>,
    baseline: Option<&HashMap<String, BenchmarkAggregateMetrics>>,
) -> Vec<EvalNamedMetricComparison> {
    let Some(baseline) = baseline else {
        return Vec::new();
    };

    let mut comparisons = current
        .iter()
        .filter_map(|(name, current_metrics)| {
            baseline.get(name).map(|baseline_metrics| {
                build_comparison(name.clone(), current_metrics, baseline_metrics)
            })
        })
        .collect::<Vec<_>>();
    comparisons.sort_by(|left, right| left.name.cmp(&right.name));
    comparisons
}

pub(in super::super) fn build_verification_health(
    results: &[EvalFixtureResult],
) -> Option<EvalVerificationHealth> {
    let warnings_total = results
        .iter()
        .map(|result| result.warnings.len())
        .sum::<usize>();
    if warnings_total == 0 {
        return None;
    }

    let mut health = EvalVerificationHealth {
        warnings_total,
        fixtures_with_warnings: results
            .iter()
            .filter(|result| !result.warnings.is_empty())
            .count(),
        ..Default::default()
    };

    for warning in results.iter().flat_map(|result| &result.warnings) {
        let lower = warning.to_ascii_lowercase();
        if lower.contains("verification fail-open kept") {
            health.fail_open_warning_count += 1;
        }
        if lower.contains("unparseable verifier output") {
            health.parse_failure_count += 1;
        }
        if lower.contains("verifier request error") {
            health.request_failure_count += 1;
        }
    }

    Some(health)
}

fn build_comparison(
    name: String,
    current: &BenchmarkAggregateMetrics,
    baseline: &BenchmarkAggregateMetrics,
) -> EvalNamedMetricComparison {
    EvalNamedMetricComparison {
        name,
        current_micro_f1: current.micro_f1,
        baseline_micro_f1: baseline.micro_f1,
        micro_f1_delta: current.micro_f1 - baseline.micro_f1,
        current_weighted_score: current.weighted_score,
        baseline_weighted_score: baseline.weighted_score,
        weighted_score_delta: current.weighted_score - baseline.weighted_score,
        current_fixture_count: current.fixture_count,
        baseline_fixture_count: baseline.fixture_count,
    }
}

#[cfg(test)]
mod tests {
    use crate::core::eval_benchmarks::AggregateMetrics;

    use super::*;

    fn metrics(micro_f1: f32, weighted_score: f32, fixture_count: usize) -> AggregateMetrics {
        AggregateMetrics {
            micro_f1,
            weighted_score,
            fixture_count,
            ..Default::default()
        }
    }

    #[test]
    fn build_named_breakdown_comparisons_intersects_current_and_baseline() {
        let current = HashMap::from([
            ("bug".to_string(), metrics(0.7, 0.72, 2)),
            ("security".to_string(), metrics(0.9, 0.93, 3)),
        ]);
        let baseline = HashMap::from([
            ("security".to_string(), metrics(0.95, 0.96, 3)),
            ("style".to_string(), metrics(0.8, 0.81, 1)),
        ]);

        let comparisons = build_named_breakdown_comparisons(&current, Some(&baseline));

        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].name, "security");
        assert!((comparisons[0].micro_f1_delta + 0.05).abs() < f32::EPSILON);
    }

    #[test]
    fn build_verification_health_counts_fail_open_signals() {
        let results = vec![
            EvalFixtureResult {
                fixture: "suite/a".to_string(),
                suite: Some("suite".to_string()),
                passed: true,
                total_comments: 1,
                required_matches: 1,
                required_total: 1,
                benchmark_metrics: None,
                suite_thresholds: None,
                difficulty: None,
                metadata: None,
                rule_metrics: vec![],
                rule_summary: None,
                warnings: vec![
                    "verification fail-open kept 1 comment(s) after verifier request error: boom"
                        .to_string(),
                    "verification fail-open kept 1 comment(s) after unparseable verifier output"
                        .to_string(),
                ],
                artifact_path: None,
                failures: vec![],
            },
            EvalFixtureResult {
                fixture: "suite/b".to_string(),
                suite: Some("suite".to_string()),
                passed: true,
                total_comments: 1,
                required_matches: 1,
                required_total: 1,
                benchmark_metrics: None,
                suite_thresholds: None,
                difficulty: None,
                metadata: None,
                rule_metrics: vec![],
                rule_summary: None,
                warnings: vec![],
                artifact_path: None,
                failures: vec![],
            },
        ];

        let health = build_verification_health(&results).unwrap();
        assert_eq!(health.warnings_total, 2);
        assert_eq!(health.fixtures_with_warnings, 1);
        assert_eq!(health.fail_open_warning_count, 2);
        assert_eq!(health.parse_failure_count, 1);
        assert_eq!(health.request_failure_count, 1);
    }
}
