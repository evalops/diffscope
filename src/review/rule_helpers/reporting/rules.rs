use std::collections::HashMap;

use crate::core;

#[derive(Debug, Default, Clone, Copy)]
pub struct RuleHitBreakdown {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub suggestions: usize,
}

pub fn normalize_rule_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

pub fn summarize_rule_hits(
    comments: &[core::Comment],
    max_rules: usize,
    rule_priority: &[String],
) -> Vec<(String, RuleHitBreakdown)> {
    let mut by_rule: HashMap<String, RuleHitBreakdown> = HashMap::new();
    for comment in comments {
        let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) else {
            continue;
        };
        let hit = by_rule.entry(rule_id).or_default();
        hit.total = hit.total.saturating_add(1);
        match comment.severity {
            core::comment::Severity::Error => hit.errors = hit.errors.saturating_add(1),
            core::comment::Severity::Warning => hit.warnings = hit.warnings.saturating_add(1),
            core::comment::Severity::Info => hit.infos = hit.infos.saturating_add(1),
            core::comment::Severity::Suggestion => {
                hit.suggestions = hit.suggestions.saturating_add(1);
            }
        }
    }

    let priority_rank = build_rule_priority_rank(rule_priority);
    let mut rows = by_rule.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        let left_rank = priority_rank.get(&left.0).copied().unwrap_or(usize::MAX);
        let right_rank = priority_rank.get(&right.0).copied().unwrap_or(usize::MAX);
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.1.total.cmp(&left.1.total))
            .then_with(|| right.1.errors.cmp(&left.1.errors))
            .then_with(|| left.0.cmp(&right.0))
    });
    rows.truncate(max_rules);
    rows
}

pub fn build_rule_priority_rank(rule_priority: &[String]) -> HashMap<String, usize> {
    let mut by_rule = HashMap::new();
    for (idx, rule_id) in rule_priority.iter().enumerate() {
        let normalized = rule_id.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        by_rule.entry(normalized).or_insert(idx);
    }
    by_rule
}
