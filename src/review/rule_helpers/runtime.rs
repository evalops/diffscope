use std::collections::HashMap;

use crate::core;
use crate::parsing::parse_smart_category;

pub fn inject_rule_context(
    diff: &core::UnifiedDiff,
    active_rules: &[core::ReviewRule],
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    if active_rules.is_empty() {
        return;
    }

    let mut lines = Vec::new();
    lines.push(
        "Active review rules. If a finding maps to a rule, include `RULE: <id>` in the issue."
            .to_string(),
    );

    for rule in active_rules {
        let mut attrs = Vec::new();
        if let Some(scope) = &rule.scope {
            attrs.push(format!("scope={}", scope));
        }
        if let Some(severity) = &rule.severity {
            attrs.push(format!("severity={}", severity));
        }
        if let Some(category) = &rule.category {
            attrs.push(format!("category={}", category));
        }
        if !rule.tags.is_empty() {
            attrs.push(format!("tags={}", rule.tags.join("|")));
        }

        if attrs.is_empty() {
            lines.push(format!("- {}: {}", rule.id, rule.description));
        } else {
            lines.push(format!(
                "- {}: {} ({})",
                rule.id,
                rule.description,
                attrs.join(", ")
            ));
        }
    }

    context_chunks.push(
        core::LLMContextChunk::documentation(diff.file_path.clone(), lines.join("\n"))
            .with_provenance(core::ContextProvenance::ActiveReviewRules),
    );
}

pub fn apply_rule_overrides(
    mut comments: Vec<core::Comment>,
    active_rules: &[core::ReviewRule],
) -> Vec<core::Comment> {
    if comments.is_empty() || active_rules.is_empty() {
        return comments;
    }

    let mut by_id = HashMap::new();
    for rule in active_rules {
        by_id.insert(rule.id.to_ascii_lowercase(), rule);
    }

    for comment in &mut comments {
        let Some(rule_id) = comment.rule_id.clone() else {
            continue;
        };
        let key = rule_id.trim().to_ascii_lowercase();
        let Some(rule) = by_id.get(&key) else {
            continue;
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

        let marker = format!("rule:{}", rule.id);
        if !comment.tags.iter().any(|tag| tag == &marker) {
            comment.tags.push(marker);
        }
        for tag in &rule.tags {
            if !comment.tags.iter().any(|existing| existing == tag) {
                comment.tags.push(tag.clone());
            }
        }
        comment.confidence = comment.confidence.max(0.8);
    }

    comments
}

fn parse_rule_severity_override(value: &str) -> Option<core::comment::Severity> {
    match value.trim().to_ascii_lowercase().as_str() {
        "critical" | "error" => Some(core::comment::Severity::Error),
        "high" | "warning" | "warn" => Some(core::comment::Severity::Warning),
        "medium" | "info" | "informational" => Some(core::comment::Severity::Info),
        "low" | "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

fn parse_rule_category_override(value: &str) -> Option<core::comment::Category> {
    parse_smart_category(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn build_comment(
        id: &str,
        category: core::comment::Category,
        severity: core::comment::Severity,
        confidence: f32,
    ) -> core::Comment {
        core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test comment".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        }
    }

    #[test]
    fn apply_rule_overrides_sets_severity_and_category() {
        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        );
        comment.rule_id = Some("sec.xss".to_string());

        let rules = vec![core::ReviewRule {
            source: String::new(),
            id: "sec.xss".to_string(),
            description: "XSS check".to_string(),
            severity: Some("error".to_string()),
            category: Some("security".to_string()),
            scope: None,
            tags: vec!["owasp".to_string()],
        }];

        let result = apply_rule_overrides(vec![comment], &rules);
        assert_eq!(result[0].severity, core::comment::Severity::Error);
        assert_eq!(result[0].category, core::comment::Category::Security);
        assert!(result[0].tags.contains(&"rule:sec.xss".to_string()));
        assert!(result[0].tags.contains(&"owasp".to_string()));
        assert!(result[0].confidence >= 0.8);
    }

    #[test]
    fn apply_rule_overrides_no_matching_rule() {
        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        );
        comment.rule_id = Some("other.rule".to_string());

        let rules = vec![core::ReviewRule {
            source: String::new(),
            id: "sec.xss".to_string(),
            description: "XSS check".to_string(),
            severity: None,
            category: None,
            scope: None,
            tags: vec![],
        }];

        let result = apply_rule_overrides(vec![comment], &rules);
        assert_eq!(result[0].severity, core::comment::Severity::Info);
    }
}
