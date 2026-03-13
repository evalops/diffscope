use std::collections::HashMap;

use super::super::super::EvalRuleMetrics;

pub(super) fn build_rule_f1_map(metrics: &[EvalRuleMetrics]) -> HashMap<String, f32> {
    let mut by_rule = HashMap::new();
    for metric in metrics {
        by_rule.insert(metric.rule_id.to_ascii_lowercase(), metric.f1);
    }
    by_rule
}
