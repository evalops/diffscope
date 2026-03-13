use std::collections::HashMap;

use crate::core::eval_benchmarks::{
    evaluate_against_thresholds, AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkResult,
    Difficulty,
};

use super::super::{EvalFixtureResult, EvalSuiteResult};

pub(in super::super) fn build_suite_results(results: &[EvalFixtureResult]) -> Vec<EvalSuiteResult> {
    let mut grouped: HashMap<String, Vec<&EvalFixtureResult>> = HashMap::new();
    for result in results {
        if let (Some(suite), Some(_)) = (&result.suite, &result.benchmark_metrics) {
            grouped.entry(suite.clone()).or_default().push(result);
        }
    }

    let mut suites = Vec::new();
    for (suite_name, suite_results) in grouped {
        let mut fixture_results = Vec::new();
        let mut weights = Vec::new();
        let mut thresholds = None;

        for result in suite_results {
            if let Some(metrics) = result.benchmark_metrics.as_ref() {
                fixture_results.push(metrics);
                weights.push(
                    result
                        .difficulty
                        .as_ref()
                        .map(Difficulty::weight)
                        .unwrap_or(1.0),
                );
                if thresholds.is_none() {
                    thresholds = result.suite_thresholds.clone();
                }
            }
        }

        let aggregate = BenchmarkAggregateMetrics::compute(&fixture_results, Some(&weights));
        let (thresholds_enforced, threshold_pass, threshold_failures) =
            if let Some(thresholds) = thresholds.as_ref() {
                let benchmark_result = BenchmarkResult {
                    suite_name: suite_name.clone(),
                    fixture_results: fixture_results
                        .iter()
                        .map(|result| (*result).clone())
                        .collect(),
                    aggregate: aggregate.clone(),
                    by_category: HashMap::new(),
                    by_difficulty: HashMap::new(),
                    threshold_pass: true,
                    threshold_failures: Vec::new(),
                    timestamp: String::new(),
                };
                let (passed, failures) = evaluate_against_thresholds(&benchmark_result, thresholds);
                (true, passed, failures)
            } else {
                (false, true, Vec::new())
            };

        suites.push(EvalSuiteResult {
            suite: suite_name,
            fixture_count: fixture_results.len(),
            aggregate,
            thresholds_enforced,
            threshold_pass,
            threshold_failures,
        });
    }

    suites.sort_by(|left, right| left.suite.cmp(&right.suite));
    suites
}

pub(in super::super) fn collect_suite_threshold_failures(
    suites: &[EvalSuiteResult],
) -> Vec<String> {
    let mut failures = Vec::new();
    for suite in suites {
        for failure in &suite.threshold_failures {
            failures.push(format!("suite '{}' {}", suite.suite, failure));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::eval_benchmarks::{BenchmarkThresholds, Difficulty, FixtureResult};

    #[test]
    fn test_build_suite_results_applies_pack_thresholds() {
        let results = vec![EvalFixtureResult {
            fixture: "community/sql-injection".to_string(),
            suite: Some("community".to_string()),
            passed: false,
            total_comments: 2,
            required_matches: 1,
            required_total: 1,
            benchmark_metrics: Some(FixtureResult {
                fixture_name: "community/sql-injection".to_string(),
                true_positives: 1,
                false_positives: 1,
                false_negatives: 0,
                true_negatives: 0,
                precision: 0.5,
                recall: 1.0,
                f1: 0.6666667,
                passed: false,
                details: vec![],
            }),
            suite_thresholds: Some(BenchmarkThresholds {
                min_precision: 0.9,
                min_recall: 0.9,
                min_f1: 0.9,
                max_false_positive_rate: 0.0,
                min_weighted_score: 0.95,
            }),
            difficulty: Some(Difficulty::Hard),
            rule_metrics: vec![],
            rule_summary: None,
            failures: vec!["missing finding".to_string()],
        }];

        let suites = build_suite_results(&results);

        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].suite, "community");
        assert!(!suites[0].threshold_pass);
        assert!(!suites[0].threshold_failures.is_empty());
    }
}
