use std::collections::HashMap;

use crate::core::eval_benchmarks::{
    evaluate_against_thresholds, AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkResult,
    Difficulty,
};

use super::super::{EvalFixtureResult, EvalSuiteResult};

pub(in super::super) struct EvalBenchmarkBreakdowns {
    pub(in super::super) by_category: HashMap<String, BenchmarkAggregateMetrics>,
    pub(in super::super) by_language: HashMap<String, BenchmarkAggregateMetrics>,
    pub(in super::super) by_difficulty: HashMap<String, BenchmarkAggregateMetrics>,
}

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

pub(in super::super) fn build_benchmark_breakdowns(
    results: &[EvalFixtureResult],
) -> EvalBenchmarkBreakdowns {
    EvalBenchmarkBreakdowns {
        by_category: aggregate_breakdown(results, |result| {
            result
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.category.clone())
        }),
        by_language: aggregate_breakdown(results, |result| {
            result
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.language.clone())
        }),
        by_difficulty: aggregate_breakdown(results, |result| {
            result
                .difficulty
                .as_ref()
                .map(|difficulty| difficulty_label(difficulty).to_string())
        }),
    }
}

fn aggregate_breakdown<F>(
    results: &[EvalFixtureResult],
    key_fn: F,
) -> HashMap<String, BenchmarkAggregateMetrics>
where
    F: Fn(&EvalFixtureResult) -> Option<String>,
{
    let mut grouped: HashMap<String, Vec<(&crate::core::eval_benchmarks::FixtureResult, f32)>> =
        HashMap::new();

    for result in results {
        let Some(metrics) = result.benchmark_metrics.as_ref() else {
            continue;
        };
        let Some(key) = key_fn(result) else {
            continue;
        };
        let weight = result
            .difficulty
            .as_ref()
            .map(Difficulty::weight)
            .unwrap_or(1.0);
        grouped.entry(key).or_default().push((metrics, weight));
    }

    let mut aggregates = HashMap::new();
    for (key, grouped_results) in grouped {
        let fixture_results = grouped_results
            .iter()
            .map(|(result, _)| *result)
            .collect::<Vec<_>>();
        let weights = grouped_results
            .iter()
            .map(|(_, weight)| *weight)
            .collect::<Vec<_>>();
        aggregates.insert(
            key,
            BenchmarkAggregateMetrics::compute(&fixture_results, Some(&weights)),
        );
    }

    aggregates
}

fn difficulty_label(difficulty: &Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::eval::EvalFixtureMetadata;
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
            metadata: None,
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

    #[test]
    fn test_build_benchmark_breakdowns_groups_by_metadata() {
        let results = vec![
            EvalFixtureResult {
                fixture: "suite/a".to_string(),
                suite: Some("suite".to_string()),
                passed: true,
                total_comments: 1,
                required_matches: 1,
                required_total: 1,
                benchmark_metrics: Some(FixtureResult::compute("suite/a", 1, 0, 1, 0, 0)),
                suite_thresholds: None,
                difficulty: Some(Difficulty::Hard),
                metadata: Some(EvalFixtureMetadata {
                    category: Some("security".to_string()),
                    language: Some("rust".to_string()),
                    source: None,
                    description: None,
                }),
                rule_metrics: vec![],
                rule_summary: None,
                failures: vec![],
            },
            EvalFixtureResult {
                fixture: "suite/b".to_string(),
                suite: Some("suite".to_string()),
                passed: true,
                total_comments: 1,
                required_matches: 1,
                required_total: 1,
                benchmark_metrics: Some(FixtureResult::compute("suite/b", 1, 0, 1, 0, 0)),
                suite_thresholds: None,
                difficulty: Some(Difficulty::Medium),
                metadata: Some(EvalFixtureMetadata {
                    category: Some("performance".to_string()),
                    language: Some("python".to_string()),
                    source: None,
                    description: None,
                }),
                rule_metrics: vec![],
                rule_summary: None,
                failures: vec![],
            },
        ];

        let breakdowns = build_benchmark_breakdowns(&results);

        assert_eq!(
            breakdowns
                .by_category
                .get("security")
                .map(|metrics| metrics.fixture_count),
            Some(1)
        );
        assert_eq!(
            breakdowns
                .by_language
                .get("python")
                .map(|metrics| metrics.fixture_count),
            Some(1)
        );
        assert_eq!(
            breakdowns
                .by_difficulty
                .get("hard")
                .map(|metrics| metrics.fixture_count),
            Some(1)
        );
    }
}
