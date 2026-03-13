use anyhow::Result;
use std::collections::HashMap;

use super::{EvalReport, EvalRuleMetrics};

#[derive(Debug, Clone)]
pub(super) struct EvalThresholdOptions {
    pub(super) max_micro_f1_drop: Option<f32>,
    pub(super) min_micro_f1: Option<f32>,
    pub(super) min_macro_f1: Option<f32>,
    pub(super) min_rule_f1: Vec<EvalRuleThreshold>,
    pub(super) max_rule_f1_drop: Vec<EvalRuleThreshold>,
}

#[derive(Debug, Clone)]
pub(super) struct EvalRuleThreshold {
    pub(super) rule_id: String,
    pub(super) value: f32,
}

pub(super) fn parse_rule_threshold_args(
    values: &[String],
    label: &str,
) -> Result<Vec<EvalRuleThreshold>> {
    let mut parsed = Vec::new();
    for raw in values {
        let Some((rule_id, value)) = raw.split_once('=') else {
            anyhow::bail!("Invalid {} entry '{}': expected rule_id=value", label, raw);
        };
        let rule_id = rule_id.trim().to_ascii_lowercase();
        if rule_id.is_empty() {
            anyhow::bail!("Invalid {} entry '{}': empty rule id", label, raw);
        }
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid {} entry '{}': invalid float", label, raw))?;
        if !(0.0..=1.0).contains(&value) {
            anyhow::bail!(
                "Invalid {} entry '{}': value must be between 0.0 and 1.0",
                label,
                raw
            );
        }
        parsed.push(EvalRuleThreshold { rule_id, value });
    }
    Ok(parsed)
}

pub(super) fn evaluate_eval_thresholds(
    current: &EvalReport,
    baseline: Option<&EvalReport>,
    options: &EvalThresholdOptions,
) -> Vec<String> {
    let mut failures = Vec::new();
    let current_micro_f1 = current
        .rule_summary
        .map(|summary| summary.micro_f1)
        .unwrap_or(0.0);
    let current_macro_f1 = current
        .rule_summary
        .map(|summary| summary.macro_f1)
        .unwrap_or(0.0);

    if let Some(threshold) = options.min_micro_f1 {
        let threshold = threshold.clamp(0.0, 1.0);
        if current_micro_f1 < threshold {
            failures.push(format!(
                "micro-F1 {:.3} is below minimum {:.3}",
                current_micro_f1, threshold
            ));
        }
    }

    if let Some(threshold) = options.min_macro_f1 {
        let threshold = threshold.clamp(0.0, 1.0);
        if current_macro_f1 < threshold {
            failures.push(format!(
                "macro-F1 {:.3} is below minimum {:.3}",
                current_macro_f1, threshold
            ));
        }
    }

    let current_by_rule = build_rule_f1_map(&current.rule_metrics);
    for threshold in &options.min_rule_f1 {
        let current = current_by_rule
            .get(&threshold.rule_id)
            .copied()
            .unwrap_or(0.0);
        if current < threshold.value {
            failures.push(format!(
                "rule '{}' F1 {:.3} is below minimum {:.3}",
                threshold.rule_id, current, threshold.value
            ));
        }
    }

    if options.max_micro_f1_drop.is_some() || !options.max_rule_f1_drop.is_empty() {
        let Some(baseline) = baseline else {
            failures.push(
                "baseline report is required for drop-based thresholds (--baseline)".to_string(),
            );
            return failures;
        };

        let baseline_summary = baseline.rule_summary.unwrap_or_default();
        if let Some(max_drop) = options.max_micro_f1_drop {
            let max_drop = max_drop.clamp(0.0, 1.0);
            let drop = (baseline_summary.micro_f1 - current_micro_f1).max(0.0);
            if drop > max_drop {
                failures.push(format!(
                    "micro-F1 drop {:.3} exceeded max {:.3} (baseline {:.3} -> current {:.3})",
                    drop, max_drop, baseline_summary.micro_f1, current_micro_f1
                ));
            }
        }

        if !options.max_rule_f1_drop.is_empty() {
            let baseline_by_rule = build_rule_f1_map(&baseline.rule_metrics);
            for threshold in &options.max_rule_f1_drop {
                let baseline_f1 = baseline_by_rule
                    .get(&threshold.rule_id)
                    .copied()
                    .unwrap_or(0.0);
                let current_f1 = current_by_rule
                    .get(&threshold.rule_id)
                    .copied()
                    .unwrap_or(0.0);
                let drop = (baseline_f1 - current_f1).max(0.0);
                if drop > threshold.value {
                    failures.push(format!(
                        "rule '{}' F1 drop {:.3} exceeded max {:.3} (baseline {:.3} -> current {:.3})",
                        threshold.rule_id, drop, threshold.value, baseline_f1, current_f1
                    ));
                }
            }
        }
    }

    failures
}

fn build_rule_f1_map(metrics: &[EvalRuleMetrics]) -> HashMap<String, f32> {
    let mut by_rule = HashMap::new();
    for metric in metrics {
        by_rule.insert(metric.rule_id.to_ascii_lowercase(), metric.f1);
    }
    by_rule
}

#[cfg(test)]
mod tests {
    use super::super::{EvalReport, EvalRuleMetrics, EvalRuleScoreSummary};
    use super::*;

    #[test]
    fn test_evaluate_eval_thresholds_requires_baseline_for_drop_checks() {
        let report = EvalReport {
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
            threshold_failures: vec![],
            results: vec![],
        };
        let baseline = EvalReport {
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
