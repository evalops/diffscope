#[path = "metrics/comparisons.rs"]
mod comparisons;
#[path = "metrics/lifecycle.rs"]
mod lifecycle;
#[path = "metrics/rules.rs"]
mod rules;
#[path = "metrics/suites.rs"]
mod suites;

use super::EvalReport;

pub(super) use comparisons::{
    build_named_breakdown_comparisons, build_suite_comparisons, build_verification_health,
};
pub(super) use lifecycle::build_lifecycle_accuracy;
pub(super) use rules::{aggregate_rule_metrics, compute_rule_metrics, summarize_rule_metrics};
pub(super) use suites::{
    build_benchmark_breakdowns, build_overall_benchmark_summary, build_suite_results,
    collect_suite_threshold_failures,
};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct EvalUsefulnessSignals {
    pub micro_f1: Option<f32>,
    pub weighted_score: Option<f32>,
    pub pass_rate: f32,
    pub verification_health: Option<f32>,
    pub lifecycle_accuracy: Option<f32>,
}

pub(super) fn build_report_usefulness_signals(report: &EvalReport) -> EvalUsefulnessSignals {
    EvalUsefulnessSignals {
        micro_f1: report
            .benchmark_summary
            .as_ref()
            .map(|metrics| metrics.micro_f1),
        weighted_score: report
            .benchmark_summary
            .as_ref()
            .map(|metrics| metrics.weighted_score),
        pass_rate: ratio(report.fixtures_passed, report.fixtures_total),
        verification_health: report
            .verification_health
            .as_ref()
            .map(|health| health.verified_pct),
        lifecycle_accuracy: build_lifecycle_accuracy(&report.results).map(|accuracy| accuracy.rate),
    }
}

pub(super) fn compute_usefulness_score(signals: EvalUsefulnessSignals) -> f32 {
    const MICRO_F1_WEIGHT: f32 = 0.20;
    const WEIGHTED_SCORE_WEIGHT: f32 = 0.35;
    const PASS_RATE_WEIGHT: f32 = 0.25;
    const VERIFICATION_HEALTH_WEIGHT: f32 = 0.10;
    const LIFECYCLE_ACCURACY_WEIGHT: f32 = 0.10;

    let mut weighted_sum = signals.pass_rate * PASS_RATE_WEIGHT;
    let mut total_weight = PASS_RATE_WEIGHT;

    for (value, weight) in [
        (signals.micro_f1, MICRO_F1_WEIGHT),
        (signals.weighted_score, WEIGHTED_SCORE_WEIGHT),
        (signals.verification_health, VERIFICATION_HEALTH_WEIGHT),
        (signals.lifecycle_accuracy, LIFECYCLE_ACCURACY_WEIGHT),
    ] {
        if let Some(value) = value {
            weighted_sum += value * weight;
            total_weight += weight;
        }
    }

    if total_weight == 0.0 {
        0.0
    } else {
        weighted_sum / total_weight
    }
}

fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::eval::{
        EvalFixtureResult, EvalReport, EvalRuleMetrics, EvalRunMetadata, EvalVerificationHealth,
    };
    use crate::core::eval_benchmarks::{AggregateMetrics, FixtureResult};

    fn sample_report() -> EvalReport {
        EvalReport {
            run: EvalRunMetadata {
                model: "anthropic/claude-opus-4.1".to_string(),
                ..Default::default()
            },
            fixtures_total: 4,
            fixtures_passed: 3,
            fixtures_failed: 1,
            benchmark_summary: Some(AggregateMetrics {
                fixture_count: 4,
                micro_f1: 0.7,
                weighted_score: 0.8,
                ..Default::default()
            }),
            verification_health: Some(EvalVerificationHealth {
                verified_checks: 9,
                total_checks: 10,
                verified_pct: 0.9,
                warnings_total: 1,
                fixtures_with_warnings: 1,
                fail_open_warning_count: 0,
                parse_failure_count: 0,
                request_failure_count: 0,
            }),
            results: vec![EvalFixtureResult {
                passed: true,
                benchmark_metrics: Some(FixtureResult::compute("suite/sample", 1, 0, 1, 0, 0)),
                rule_metrics: vec![EvalRuleMetrics {
                    rule_id: "bug.lifecycle.context-only-addressed".to_string(),
                    expected: 1,
                    predicted: 1,
                    true_positives: 1,
                    false_positives: 0,
                    false_negatives: 0,
                    precision: 1.0,
                    recall: 1.0,
                    f1: 1.0,
                }],
                ..Default::default()
            }],
            rule_metrics: vec![],
            rule_summary: None,
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            warnings: vec![],
            threshold_failures: vec![],
        }
    }

    #[test]
    fn build_report_usefulness_signals_collects_report_metrics() {
        let signals = build_report_usefulness_signals(&sample_report());

        assert_eq!(signals.micro_f1, Some(0.7));
        assert_eq!(signals.weighted_score, Some(0.8));
        assert!((signals.pass_rate - 0.75).abs() < f32::EPSILON);
        assert_eq!(signals.verification_health, Some(0.9));
        assert_eq!(signals.lifecycle_accuracy, Some(1.0));
    }

    #[test]
    fn compute_usefulness_score_blends_available_signals() {
        let score = compute_usefulness_score(EvalUsefulnessSignals {
            micro_f1: Some(0.6),
            weighted_score: Some(0.8),
            pass_rate: 0.5,
            verification_health: Some(1.0),
            lifecycle_accuracy: Some(0.25),
        });

        assert!((score - 0.65).abs() < f32::EPSILON);
    }
}
