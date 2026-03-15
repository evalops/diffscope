use std::collections::HashMap;
use std::path::Path;

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

    if let Some(pattern_repository_tag) = pattern_repository_source_tag(&rule.source) {
        if !comment
            .tags
            .iter()
            .any(|existing| existing == "pattern-repository")
        {
            comment.tags.push("pattern-repository".to_string());
        }
        if !comment
            .tags
            .iter()
            .any(|existing| existing == &pattern_repository_tag)
        {
            comment.tags.push(pattern_repository_tag);
        }
    }

    for tag in &rule.tags {
        if !comment.tags.iter().any(|existing| existing == tag) {
            comment.tags.push(tag.clone());
        }
    }
}

fn pattern_repository_source_tag(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("repository") {
        return None;
    }

    Some(format!(
        "pattern-repository:{}",
        pattern_repository_source_label(trimmed)
    ))
}

fn pattern_repository_source_label(source: &str) -> String {
    let trimmed = source.trim().trim_end_matches('/').trim_end_matches(".git");

    if !trimmed.contains("://") && !trimmed.contains('@') {
        let parts = trimmed
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();

        if parts.len() == 2 {
            return format!("{}/{}", parts[0], parts[1]);
        }
    }

    if !trimmed.contains("://") && !trimmed.contains('@') {
        if let Some(name) = Path::new(trimmed)
            .file_name()
            .and_then(|value| value.to_str())
        {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }

    let normalized = trimmed.replace(':', "/");
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty() && !part.ends_with(':'))
        .collect::<Vec<_>>();

    match parts.as_slice() {
        [.., owner, repo] => format!("{owner}/{repo}"),
        [single] => (*single).to_string(),
        [] => "external".to_string(),
    }
}
