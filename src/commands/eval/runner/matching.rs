#[path = "matching/required.rs"]
mod required;
#[path = "matching/rules.rs"]
mod rules;
#[path = "matching/unexpected.rs"]
mod unexpected;

use std::collections::HashSet;

use crate::core;

use super::super::{EvalExpectations, EvalRuleMetrics, EvalRuleScoreSummary};
use required::collect_required_matches;
use rules::build_rule_match_metrics;
use unexpected::collect_unexpected_matches;

#[derive(Debug, Clone)]
pub(super) struct FixtureMatchSummary {
    pub(super) failures: Vec<String>,
    pub(super) required_matches: usize,
    pub(super) required_total: usize,
    pub(super) rule_metrics: Vec<EvalRuleMetrics>,
    pub(super) rule_summary: Option<EvalRuleScoreSummary>,
    pub(super) used_comment_indices: HashSet<usize>,
    pub(super) unexpected_comment_indices: HashSet<usize>,
}

pub(super) fn evaluate_fixture_expectations(
    expectations: &EvalExpectations,
    comments: &[core::Comment],
) -> FixtureMatchSummary {
    let required_total = expectations.must_find.len();
    let required = collect_required_matches(expectations, comments);
    let unexpected = collect_unexpected_matches(expectations, comments);
    let (rule_metrics, rule_summary) =
        build_rule_match_metrics(expectations, comments, &required.matched_pairs);

    FixtureMatchSummary {
        failures: required
            .failures
            .into_iter()
            .chain(unexpected.failures)
            .collect(),
        required_matches: required.required_matches,
        required_total,
        rule_metrics,
        rule_summary,
        used_comment_indices: required.used_comment_indices,
        unexpected_comment_indices: unexpected.unexpected_comment_indices,
    }
}
