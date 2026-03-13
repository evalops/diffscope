use std::collections::HashMap;

use crate::core;
use crate::core::eval_benchmarks::{
    evaluate_against_thresholds, AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkResult,
    Difficulty,
};
use crate::review::normalize_rule_id;

use super::{
    EvalFixtureResult, EvalPattern, EvalRuleMetrics, EvalRuleScoreSummary, EvalSuiteResult,
};

#[derive(Debug, Default, Clone, Copy)]
struct RuleMetricCounts {
    expected: usize,
    predicted: usize,
    true_positives: usize,
}

pub(super) fn build_suite_results(results: &[EvalFixtureResult]) -> Vec<EvalSuiteResult> {
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

pub(super) fn collect_suite_threshold_failures(suites: &[EvalSuiteResult]) -> Vec<String> {
    let mut failures = Vec::new();
    for suite in suites {
        for failure in &suite.threshold_failures {
            failures.push(format!("suite '{}' {}", suite.suite, failure));
        }
    }
    failures
}

pub(super) fn compute_rule_metrics(
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();

    for pattern in expected_patterns {
        if let Some(rule_id) = pattern.normalized_rule_id() {
            counts_by_rule.entry(rule_id).or_default().expected += 1;
        }
    }

    for comment in comments {
        if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
            counts_by_rule.entry(rule_id).or_default().predicted += 1;
        }
    }

    for (expected_idx, comment_idx) in matched_pairs {
        let expected_rule = expected_patterns
            .get(*expected_idx)
            .and_then(EvalPattern::normalized_rule_id);
        let predicted_rule = comments
            .get(*comment_idx)
            .and_then(|comment| normalize_rule_id(comment.rule_id.as_deref()));
        if let (Some(expected_rule), Some(predicted_rule)) = (expected_rule, predicted_rule) {
            if expected_rule == predicted_rule {
                counts_by_rule
                    .entry(expected_rule)
                    .or_default()
                    .true_positives += 1;
            }
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

pub(super) fn aggregate_rule_metrics(results: &[EvalFixtureResult]) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();
    for result in results {
        for metric in &result.rule_metrics {
            let counts = counts_by_rule.entry(metric.rule_id.clone()).or_default();
            counts.expected = counts.expected.saturating_add(metric.expected);
            counts.predicted = counts.predicted.saturating_add(metric.predicted);
            counts.true_positives = counts.true_positives.saturating_add(metric.true_positives);
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

fn build_rule_metrics_from_counts(
    counts_by_rule: &HashMap<String, RuleMetricCounts>,
) -> Vec<EvalRuleMetrics> {
    let mut metrics = Vec::new();
    for (rule_id, counts) in counts_by_rule {
        let false_positives = counts.predicted.saturating_sub(counts.true_positives);
        let false_negatives = counts.expected.saturating_sub(counts.true_positives);
        let precision = if counts.predicted > 0 {
            counts.true_positives as f32 / counts.predicted as f32
        } else {
            0.0
        };
        let recall = if counts.expected > 0 {
            counts.true_positives as f32 / counts.expected as f32
        } else {
            0.0
        };
        let f1 = harmonic_mean(precision, recall);

        metrics.push(EvalRuleMetrics {
            rule_id: rule_id.clone(),
            expected: counts.expected,
            predicted: counts.predicted,
            true_positives: counts.true_positives,
            false_positives,
            false_negatives,
            precision,
            recall,
            f1,
        });
    }

    metrics.sort_by(|left, right| {
        right
            .expected
            .cmp(&left.expected)
            .then_with(|| right.predicted.cmp(&left.predicted))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    metrics
}

pub(super) fn summarize_rule_metrics(metrics: &[EvalRuleMetrics]) -> Option<EvalRuleScoreSummary> {
    if metrics.is_empty() {
        return None;
    }

    let mut tp_sum = 0usize;
    let mut predicted_sum = 0usize;
    let mut expected_sum = 0usize;
    let mut precision_sum = 0.0f32;
    let mut recall_sum = 0.0f32;
    let mut f1_sum = 0.0f32;

    for metric in metrics {
        tp_sum = tp_sum.saturating_add(metric.true_positives);
        predicted_sum = predicted_sum.saturating_add(metric.predicted);
        expected_sum = expected_sum.saturating_add(metric.expected);
        precision_sum += metric.precision;
        recall_sum += metric.recall;
        f1_sum += metric.f1;
    }

    let micro_precision = if predicted_sum > 0 {
        tp_sum as f32 / predicted_sum as f32
    } else {
        0.0
    };
    let micro_recall = if expected_sum > 0 {
        tp_sum as f32 / expected_sum as f32
    } else {
        0.0
    };
    let micro_f1 = harmonic_mean(micro_precision, micro_recall);
    let count = metrics.len() as f32;

    Some(EvalRuleScoreSummary {
        micro_precision,
        micro_recall,
        micro_f1,
        macro_precision: precision_sum / count,
        macro_recall: recall_sum / count,
        macro_f1: f1_sum / count,
    })
}

fn harmonic_mean(precision: f32, recall: f32) -> f32 {
    if precision + recall <= f32::EPSILON {
        0.0
    } else {
        (2.0 * precision * recall) / (precision + recall)
    }
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
