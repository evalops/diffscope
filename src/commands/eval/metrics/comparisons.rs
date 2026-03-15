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
        .map(|result| {
            result
                .warnings
                .iter()
                .filter(|warning| is_verification_warning(warning))
                .count()
        })
        .sum::<usize>();

    let mut health = EvalVerificationHealth {
        warnings_total,
        fixtures_with_warnings: results
            .iter()
            .filter(|result| {
                result
                    .warnings
                    .iter()
                    .any(|warning| is_verification_warning(warning))
            })
            .count(),
        ..Default::default()
    };

    let mut observed_verification = false;
    for result in results {
        if let Some(report) = result.verification_report.as_ref() {
            observed_verification = true;
            for judge in &report.judges {
                health.total_checks += judge.total_comments;
                health.verified_checks += judge.passed_comments + judge.filtered_comments;
            }
        } else if result.total_comments > 0
            && result
                .warnings
                .iter()
                .any(|warning| is_verification_warning(warning))
        {
            observed_verification = true;
            health.total_checks += result.total_comments;
        }
    }

    if health.total_checks > 0 {
        health.verified_pct = health.verified_checks as f32 / health.total_checks as f32;
    }

    if !observed_verification && warnings_total == 0 {
        return None;
    }

    for warning in results.iter().flat_map(|result| &result.warnings) {
        if !is_verification_warning(warning) {
            continue;
        }
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

fn is_verification_warning(warning: &str) -> bool {
    let lower = warning.to_ascii_lowercase();
    lower.contains("verification") || lower.contains("verifier")
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
    use crate::commands::eval::{EvalVerificationJudgeReport, EvalVerificationReport};
    use crate::core::eval_benchmarks::AggregateMetrics;

    use super::*;

    fn empty_report() -> EvalReport {
        EvalReport {
            run: Default::default(),
            fixtures_total: 0,
            fixtures_passed: 0,
            fixtures_failed: 0,
            rule_metrics: vec![],
            rule_summary: None,
            benchmark_summary: None,
            suite_results: vec![],
            benchmark_by_category: HashMap::new(),
            benchmark_by_language: HashMap::new(),
            benchmark_by_difficulty: HashMap::new(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            verification_health: None,
            warnings: vec![],
            threshold_failures: vec![],
            results: vec![],
        }
    }

    fn metrics(micro_f1: f32, weighted_score: f32, fixture_count: usize) -> AggregateMetrics {
        AggregateMetrics {
            micro_f1,
            weighted_score,
            fixture_count,
            ..Default::default()
        }
    }

    #[test]
    fn build_suite_comparisons_intersects_current_and_baseline() {
        let current = vec![EvalSuiteResult {
            suite: "review-depth-infra".to_string(),
            fixture_count: 2,
            aggregate: metrics(0.8, 0.75, 2),
            thresholds_enforced: false,
            threshold_pass: true,
            threshold_failures: vec![],
        }];
        let baseline = EvalReport {
            suite_results: vec![EvalSuiteResult {
                suite: "review-depth-infra".to_string(),
                fixture_count: 2,
                aggregate: metrics(0.9, 0.85, 2),
                thresholds_enforced: false,
                threshold_pass: true,
                threshold_failures: vec![],
            }],
            ..empty_report()
        };

        let comparisons = build_suite_comparisons(&current, Some(&baseline));

        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].name, "review-depth-infra");
        assert!((comparisons[0].micro_f1_delta + 0.1).abs() < f32::EPSILON);
        assert!((comparisons[0].weighted_score_delta + 0.1).abs() < f32::EPSILON);
        assert_eq!(comparisons[0].current_fixture_count, 2);
        assert_eq!(comparisons[0].baseline_fixture_count, 2);
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
                verification_report: None,
                agent_activity: None,
                reproduction_summary: None,
                artifact_path: None,
                failures: vec![],
                cost_breakdowns: vec![],
                dag_traces: vec![],
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
                verification_report: None,
                agent_activity: None,
                reproduction_summary: None,
                artifact_path: None,
                failures: vec![],
                cost_breakdowns: vec![],
                dag_traces: vec![],
            },
        ];

        let health = build_verification_health(&results).unwrap();
        assert_eq!(health.verified_checks, 0);
        assert_eq!(health.total_checks, 1);
        assert_eq!(health.verified_pct, 0.0);
        assert_eq!(health.warnings_total, 2);
        assert_eq!(health.fixtures_with_warnings, 1);
        assert_eq!(health.fail_open_warning_count, 2);
        assert_eq!(health.parse_failure_count, 1);
        assert_eq!(health.request_failure_count, 1);
    }

    #[test]
    fn build_verification_health_returns_none_for_non_verification_warnings_only() {
        let results = vec![EvalFixtureResult {
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
            warnings: vec!["reproduction validator warning".to_string()],
            verification_report: None,
            agent_activity: None,
            reproduction_summary: None,
            artifact_path: None,
            failures: vec![],
            cost_breakdowns: vec![],
            dag_traces: vec![],
        }];

        assert!(build_verification_health(&results).is_none());
    }

    #[test]
    fn build_verification_health_detects_verifier_only_warning_text() {
        let results = vec![EvalFixtureResult {
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
            warnings: vec!["verifier request error: timeout".to_string()],
            verification_report: None,
            agent_activity: None,
            reproduction_summary: None,
            artifact_path: None,
            failures: vec![],
            cost_breakdowns: vec![],
            dag_traces: vec![],
        }];

        let health = build_verification_health(&results).unwrap();

        assert_eq!(health.warnings_total, 1);
        assert_eq!(health.fixtures_with_warnings, 1);
        assert_eq!(health.request_failure_count, 1);
        assert_eq!(health.total_checks, 1);
    }

    #[test]
    fn build_verification_health_keeps_zero_percent_when_no_checks_ran() {
        let results = vec![EvalFixtureResult {
            fixture: "suite/a".to_string(),
            suite: Some("suite".to_string()),
            passed: true,
            total_comments: 0,
            required_matches: 0,
            required_total: 0,
            benchmark_metrics: None,
            suite_thresholds: None,
            difficulty: None,
            metadata: None,
            rule_metrics: vec![],
            rule_summary: None,
            warnings: vec![],
            verification_report: Some(EvalVerificationReport {
                consensus_mode: "majority".to_string(),
                required_votes: 1,
                judge_count: 1,
                judges: vec![EvalVerificationJudgeReport {
                    model: "judge".to_string(),
                    total_comments: 0,
                    passed_comments: 0,
                    filtered_comments: 0,
                    abstained_comments: 0,
                    warnings: vec![],
                    ..Default::default()
                }],
            }),
            agent_activity: None,
            reproduction_summary: None,
            artifact_path: None,
            failures: vec![],
            cost_breakdowns: vec![],
            dag_traces: vec![],
        }];

        let health = build_verification_health(&results).unwrap();

        assert_eq!(health.total_checks, 0);
        assert_eq!(health.verified_checks, 0);
        assert_eq!(health.verified_pct, 0.0);
    }

    #[test]
    fn build_verification_health_uses_judge_reports_without_warnings() {
        let results = vec![EvalFixtureResult {
            fixture: "suite/a".to_string(),
            suite: Some("suite".to_string()),
            passed: true,
            total_comments: 5,
            required_matches: 1,
            required_total: 1,
            benchmark_metrics: None,
            suite_thresholds: None,
            difficulty: None,
            metadata: None,
            rule_metrics: vec![],
            rule_summary: None,
            warnings: vec![],
            verification_report: Some(EvalVerificationReport {
                consensus_mode: "majority".to_string(),
                required_votes: 1,
                judge_count: 1,
                judges: vec![EvalVerificationJudgeReport {
                    model: "judge".to_string(),
                    total_comments: 5,
                    passed_comments: 3,
                    filtered_comments: 1,
                    abstained_comments: 1,
                    warnings: vec![],
                    ..Default::default()
                }],
            }),
            agent_activity: None,
            reproduction_summary: None,
            artifact_path: None,
            failures: vec![],
            cost_breakdowns: vec![],
            dag_traces: vec![],
        }];

        let health = build_verification_health(&results).unwrap();

        assert_eq!(health.verified_checks, 4);
        assert_eq!(health.total_checks, 5);
        assert!((health.verified_pct - 0.8).abs() < f32::EPSILON);
        assert_eq!(health.warnings_total, 0);
        assert_eq!(health.fixtures_with_warnings, 0);
    }
}
