use std::collections::HashMap;

use super::super::EvalThresholdOptions;

pub(super) fn check_minimum_thresholds(
    current_micro_f1: f32,
    current_macro_f1: f32,
    current_by_rule: &HashMap<String, f32>,
    options: &EvalThresholdOptions,
) -> Vec<String> {
    let mut failures = Vec::new();

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

    failures
}
