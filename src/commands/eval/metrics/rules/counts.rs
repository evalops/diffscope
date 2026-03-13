use std::collections::HashMap;

use crate::core;
use crate::review::normalize_rule_id;

use super::super::super::{EvalFixtureResult, EvalPattern};

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct RuleMetricCounts {
    pub(super) expected: usize,
    pub(super) predicted: usize,
    pub(super) true_positives: usize,
}

pub(super) fn collect_rule_metric_counts(
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> HashMap<String, RuleMetricCounts> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();
    add_expected_counts(&mut counts_by_rule, expected_patterns);
    add_predicted_counts(&mut counts_by_rule, comments);
    add_true_positive_counts(
        &mut counts_by_rule,
        expected_patterns,
        comments,
        matched_pairs,
    );
    counts_by_rule
}

pub(super) fn collect_aggregate_rule_counts(
    results: &[EvalFixtureResult],
) -> HashMap<String, RuleMetricCounts> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();
    for result in results {
        for metric in &result.rule_metrics {
            let counts = counts_by_rule.entry(metric.rule_id.clone()).or_default();
            counts.expected = counts.expected.saturating_add(metric.expected);
            counts.predicted = counts.predicted.saturating_add(metric.predicted);
            counts.true_positives = counts.true_positives.saturating_add(metric.true_positives);
        }
    }
    counts_by_rule
}

fn add_expected_counts(
    counts_by_rule: &mut HashMap<String, RuleMetricCounts>,
    expected_patterns: &[EvalPattern],
) {
    for pattern in expected_patterns {
        if let Some(rule_id) = pattern.normalized_rule_ids().into_iter().next() {
            counts_by_rule.entry(rule_id).or_default().expected += 1;
        }
    }
}

fn add_predicted_counts(
    counts_by_rule: &mut HashMap<String, RuleMetricCounts>,
    comments: &[core::Comment],
) {
    for comment in comments {
        if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
            counts_by_rule.entry(rule_id).or_default().predicted += 1;
        }
    }
}

fn add_true_positive_counts(
    counts_by_rule: &mut HashMap<String, RuleMetricCounts>,
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) {
    for (expected_idx, comment_idx) in matched_pairs {
        let expected_rule_ids = expected_patterns
            .get(*expected_idx)
            .map(EvalPattern::normalized_rule_ids)
            .unwrap_or_default();
        let predicted_rule = comments
            .get(*comment_idx)
            .and_then(|comment| normalize_rule_id(comment.rule_id.as_deref()));
        if let Some(predicted_rule) = predicted_rule {
            if expected_rule_ids
                .iter()
                .any(|expected| expected == &predicted_rule)
            {
                let canonical_rule = expected_rule_ids.first().cloned().unwrap_or(predicted_rule);
                counts_by_rule
                    .entry(canonical_rule)
                    .or_default()
                    .true_positives += 1;
            }
        }
    }
}
