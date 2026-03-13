use std::collections::HashSet;

use crate::core;

use super::super::metrics::{compute_rule_metrics, summarize_rule_metrics};
use super::super::pattern::summarize_for_eval;
use super::super::{EvalExpectations, EvalRuleMetrics, EvalRuleScoreSummary};

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
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let required_total = expectations.must_find.len();
    let mut used_comment_indices = HashSet::new();
    let mut unexpected_comment_indices = HashSet::new();
    let mut matched_pairs = Vec::new();

    for (expected_idx, expected) in expectations.must_find.iter().enumerate() {
        let found = comments
            .iter()
            .enumerate()
            .find(|(comment_idx, comment)| {
                !used_comment_indices.contains(comment_idx) && expected.matches(comment)
            })
            .map(|(comment_idx, _)| comment_idx);

        if let Some(comment_idx) = found {
            used_comment_indices.insert(comment_idx);
            matched_pairs.push((expected_idx, comment_idx));
            required_matches = required_matches.saturating_add(1);
        } else {
            failures.push(format!("Missing expected finding: {}", expected.describe()));
        }
    }

    for unexpected in &expectations.must_not_find {
        if let Some((comment_idx, comment)) = comments
            .iter()
            .enumerate()
            .find(|(_, comment)| unexpected.matches(comment))
        {
            unexpected_comment_indices.insert(comment_idx);
            failures.push(format!(
                "Unexpected finding matched {}:{} '{}'",
                comment.file_path.display(),
                comment.line_number,
                summarize_for_eval(&comment.content)
            ));
        }
    }

    let rule_metrics = compute_rule_metrics(&expectations.must_find, comments, &matched_pairs);
    let rule_summary = summarize_rule_metrics(&rule_metrics);

    FixtureMatchSummary {
        failures,
        required_matches,
        required_total,
        rule_metrics,
        rule_summary,
        used_comment_indices,
        unexpected_comment_indices,
    }
}
