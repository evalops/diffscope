use std::collections::HashMap;

use crate::core;

use super::parsing::{parse_rule_category_override, parse_rule_severity_override};

type RuleIndex<'a> = HashMap<String, &'a core::ReviewRule>;

pub fn apply_rule_overrides(
    mut comments: Vec<core::Comment>,
    active_rules: &[core::ReviewRule],
) -> Vec<core::Comment> {
    if comments.is_empty() || active_rules.is_empty() {
        return comments;
    }

    let rule_index = build_rule_index(active_rules);
    for comment in &mut comments {
        apply_comment_rule_override(comment, &rule_index);
    }
    comments
}

fn build_rule_index(active_rules: &[core::ReviewRule]) -> RuleIndex<'_> {
    let mut by_id = HashMap::new();
    for rule in active_rules {
        by_id.insert(rule.id.to_ascii_lowercase(), rule);
    }
    by_id
}

fn apply_comment_rule_override(comment: &mut core::Comment, rule_index: &RuleIndex<'_>) {
    let Some(rule) = matching_rule(comment, rule_index) else {
        return;
    };

    comment.rule_id = Some(rule.id.clone());
    if let Some(severity) = rule
        .severity
        .as_deref()
        .and_then(parse_rule_severity_override)
    {
        comment.severity = severity;
    }
    if let Some(category) = rule
        .category
        .as_deref()
        .and_then(parse_rule_category_override)
    {
        comment.category = category;
    }

    apply_rule_tags(comment, rule);
    comment.confidence = comment.confidence.max(0.8);
}

fn matching_rule<'a>(
    comment: &core::Comment,
    rule_index: &'a RuleIndex<'_>,
) -> Option<&'a core::ReviewRule> {
    let key = comment.rule_id.as_deref()?.trim().to_ascii_lowercase();
    rule_index.get(&key).copied()
}

fn apply_rule_tags(comment: &mut core::Comment, rule: &core::ReviewRule) {
    let marker = format!("rule:{}", rule.id);
    if !comment.tags.iter().any(|tag| tag == &marker) {
        comment.tags.push(marker);
    }
    for tag in &rule.tags {
        if !comment.tags.iter().any(|existing| existing == tag) {
            comment.tags.push(tag.clone());
        }
    }
}
