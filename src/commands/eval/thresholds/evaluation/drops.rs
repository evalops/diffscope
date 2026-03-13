use std::collections::HashMap;

use super::super::super::{EvalNamedMetricComparison, EvalReport};
use super::super::EvalThresholdOptions;
use super::rules::build_rule_f1_map;

pub(super) fn check_drop_thresholds(
    current: &EvalReport,
    current_micro_f1: f32,
    current_by_rule: &HashMap<String, f32>,
    baseline: Option<&EvalReport>,
    options: &EvalThresholdOptions,
) -> Vec<String> {
    let mut failures = Vec::new();
    if options.max_micro_f1_drop.is_none()
        && options.max_rule_f1_drop.is_empty()
        && options.max_suite_f1_drop.is_none()
        && options.max_category_f1_drop.is_none()
        && options.max_language_f1_drop.is_none()
    {
        return failures;
    }

    let Some(baseline) = baseline else {
        failures
            .push("baseline report is required for drop-based thresholds (--baseline)".to_string());
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

    if let Some(max_drop) = options.max_suite_f1_drop {
        failures.extend(check_dimension_drop_thresholds(
            "suite",
            &current.suite_comparisons,
            max_drop,
        ));
    }
    if let Some(max_drop) = options.max_category_f1_drop {
        failures.extend(check_dimension_drop_thresholds(
            "category",
            &current.category_comparisons,
            max_drop,
        ));
    }
    if let Some(max_drop) = options.max_language_f1_drop {
        failures.extend(check_dimension_drop_thresholds(
            "language",
            &current.language_comparisons,
            max_drop,
        ));
    }

    failures
}

fn check_dimension_drop_thresholds(
    dimension: &str,
    comparisons: &[EvalNamedMetricComparison],
    threshold: f32,
) -> Vec<String> {
    let threshold = threshold.clamp(0.0, 1.0);
    comparisons
        .iter()
        .filter_map(|comparison| {
            let drop = (-comparison.micro_f1_delta).max(0.0);
            (drop > threshold).then(|| {
                format!(
                    "{} '{}' micro-F1 drop {:.3} exceeded max {:.3} (baseline {:.3} -> current {:.3})",
                    dimension,
                    comparison.name,
                    drop,
                    threshold,
                    comparison.baseline_micro_f1,
                    comparison.current_micro_f1
                )
            })
        })
        .collect()
}
