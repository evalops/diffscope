use super::super::super::EvalReport;
use super::super::EvalThresholdOptions;
use super::drops::check_drop_thresholds;
use super::minimums::check_minimum_thresholds;
use super::rules::build_rule_f1_map;

pub(in super::super::super) fn evaluate_eval_thresholds(
    current: &EvalReport,
    baseline: Option<&EvalReport>,
    options: &EvalThresholdOptions,
) -> Vec<String> {
    let current_micro_f1 = current
        .rule_summary
        .map(|summary| summary.micro_f1)
        .unwrap_or(0.0);
    let current_macro_f1 = current
        .rule_summary
        .map(|summary| summary.macro_f1)
        .unwrap_or(0.0);
    let current_by_rule = build_rule_f1_map(&current.rule_metrics);

    let mut failures = check_minimum_thresholds(
        current_micro_f1,
        current_macro_f1,
        &current_by_rule,
        options,
    );
    failures.extend(check_drop_thresholds(
        current_micro_f1,
        &current_by_rule,
        baseline,
        options,
    ));
    failures
}

#[cfg(test)]
mod tests {
    use super::super::super::super::{EvalReport, EvalRuleMetrics, EvalRuleScoreSummary};
    use super::*;
    use crate::commands::eval::thresholds::EvalRuleThreshold;

    #[test]
    fn test_evaluate_eval_thresholds_requires_baseline_for_drop_checks() {
        let report = EvalReport {
            run: Default::default(),
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![],
            rule_summary: Some(EvalRuleScoreSummary {
                micro_precision: 1.0,
                micro_recall: 1.0,
                micro_f1: 1.0,
                macro_precision: 1.0,
                macro_recall: 1.0,
                macro_f1: 1.0,
            }),
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            threshold_failures: vec![],
            results: vec![],
        };
        let options = EvalThresholdOptions {
            max_micro_f1_drop: Some(0.05),
            min_micro_f1: None,
            min_macro_f1: None,
            min_rule_f1: vec![],
            max_rule_f1_drop: vec![],
        };

        let failures = evaluate_eval_thresholds(&report, None, &options);

        assert_eq!(
            failures,
            vec!["baseline report is required for drop-based thresholds (--baseline)".to_string()]
        );
    }

    #[test]
    fn test_evaluate_eval_thresholds_checks_rule_specific_drop() {
        let current = EvalReport {
            run: Default::default(),
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: "sec.sql.injection".to_string(),
                expected: 1,
                predicted: 1,
                true_positives: 0,
                false_positives: 1,
                false_negatives: 1,
                precision: 0.0,
                recall: 0.0,
                f1: 0.0,
            }],
            rule_summary: Some(EvalRuleScoreSummary::default()),
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            threshold_failures: vec![],
            results: vec![],
        };
        let baseline = EvalReport {
            run: Default::default(),
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: "sec.sql.injection".to_string(),
                expected: 1,
                predicted: 1,
                true_positives: 1,
                false_positives: 0,
                false_negatives: 0,
                precision: 1.0,
                recall: 1.0,
                f1: 1.0,
            }],
            rule_summary: Some(EvalRuleScoreSummary::default()),
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            threshold_failures: vec![],
            results: vec![],
        };
        let options = EvalThresholdOptions {
            max_micro_f1_drop: None,
            min_micro_f1: None,
            min_macro_f1: None,
            min_rule_f1: vec![],
            max_rule_f1_drop: vec![EvalRuleThreshold {
                rule_id: "sec.sql.injection".to_string(),
                value: 0.2,
            }],
        };

        let failures = evaluate_eval_thresholds(&current, Some(&baseline), &options);

        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("sec.sql.injection"));
        assert!(failures[0].contains("exceeded max 0.200"));
    }
}
