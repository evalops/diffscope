use crate::core;

use super::super::super::metrics::{compute_rule_metrics, summarize_rule_metrics};
use super::super::super::{EvalExpectations, EvalRuleMetrics, EvalRuleScoreSummary};

pub(super) fn build_rule_match_metrics(
    expectations: &EvalExpectations,
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> (Vec<EvalRuleMetrics>, Option<EvalRuleScoreSummary>) {
    let rule_metrics = compute_rule_metrics(&expectations.must_find, comments, matched_pairs);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
    (rule_metrics, rule_summary)
}
