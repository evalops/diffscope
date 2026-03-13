use crate::core;

pub fn inject_rule_context(
    diff: &core::UnifiedDiff,
    active_rules: &[core::ReviewRule],
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    if active_rules.is_empty() {
        return;
    }

    context_chunks.push(
        core::LLMContextChunk::documentation(
            diff.file_path.clone(),
            build_rule_context_lines(active_rules),
        )
        .with_provenance(core::ContextProvenance::ActiveReviewRules),
    );
}

fn build_rule_context_lines(active_rules: &[core::ReviewRule]) -> String {
    let mut lines = Vec::new();
    lines.push(
        "Active review rules. If a finding maps to a rule, include `RULE: <id>` in the issue."
            .to_string(),
    );
    lines.extend(active_rules.iter().map(format_rule_context_line));
    lines.join("\n")
}

fn format_rule_context_line(rule: &core::ReviewRule) -> String {
    let attrs = rule_context_attributes(rule);
    if attrs.is_empty() {
        format!("- {}: {}", rule.id, rule.description)
    } else {
        format!("- {}: {} ({})", rule.id, rule.description, attrs.join(", "))
    }
}

fn rule_context_attributes(rule: &core::ReviewRule) -> Vec<String> {
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
    attrs
}
