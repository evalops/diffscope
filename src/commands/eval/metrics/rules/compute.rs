use crate::core;

use super::super::super::{EvalFixtureResult, EvalPattern, EvalRuleMetrics};
use super::build::build_rule_metrics_from_counts;
use super::counts::{collect_aggregate_rule_counts, collect_rule_metric_counts};

pub(in super::super::super) fn compute_rule_metrics(
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> Vec<EvalRuleMetrics> {
    let counts_by_rule = collect_rule_metric_counts(expected_patterns, comments, matched_pairs);
    build_rule_metrics_from_counts(&counts_by_rule)
}

pub(in super::super::super) fn aggregate_rule_metrics(
    results: &[EvalFixtureResult],
) -> Vec<EvalRuleMetrics> {
    let counts_by_rule = collect_aggregate_rule_counts(results);
    build_rule_metrics_from_counts(&counts_by_rule)
}
