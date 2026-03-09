use std::collections::{HashMap, HashSet};
use tracing::{info, warn};

use super::context_helpers::PatternRepositoryMap;
use crate::config;
use crate::core;
use crate::parsing::parse_smart_category;

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

pub fn severity_rank(severity: &core::comment::Severity) -> usize {
    match severity {
        core::comment::Severity::Error => 0,
        core::comment::Severity::Warning => 1,
        core::comment::Severity::Info => 2,
        core::comment::Severity::Suggestion => 3,
    }
}

pub fn format_top_findings_by_file(
    comments: &[core::Comment],
    max_files: usize,
    per_file: usize,
) -> String {
    if comments.is_empty() || max_files == 0 || per_file == 0 {
        return "- None\n".to_string();
    }

    let mut grouped: HashMap<String, Vec<&core::Comment>> = HashMap::new();
    for comment in comments {
        grouped
            .entry(comment.file_path.display().to_string())
            .or_default()
            .push(comment);
    }

    for file_comments in grouped.values_mut() {
        file_comments.sort_by(|left, right| {
            severity_rank(&left.severity)
                .cmp(&severity_rank(&right.severity))
                .then_with(|| left.line_number.cmp(&right.line_number))
        });
    }

    let mut rows = grouped.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .1
            .len()
            .cmp(&left.1.len())
            .then_with(|| left.0.cmp(&right.0))
    });
    rows.truncate(max_files);

    let mut out = String::new();
    for (path, file_comments) in rows {
        out.push_str(&format!(
            "- `{}` ({} issue(s))\n",
            path,
            file_comments.len()
        ));
        for comment in file_comments.into_iter().take(per_file) {
            let rule = comment
                .rule_id
                .as_deref()
                .map(|rule_id| format!(" rule:{}", rule_id))
                .unwrap_or_default();
            out.push_str(&format!(
                "  - `L{}` [{:?}{}] {}\n",
                comment.line_number, comment.severity, rule, comment.content
            ));
        }
    }
    out
}

pub fn build_pr_summary_comment_body(
    comments: &[core::Comment],
    rule_priority: &[String],
) -> String {
    let summary = core::CommentSynthesizer::generate_summary(comments);
    let mut body = String::new();
    body.push_str("## DiffScope Review Summary\n\n");
    body.push_str(&format!("- Total issues: {}\n", summary.total_comments));
    body.push_str(&format!("- Critical issues: {}\n", summary.critical_issues));
    body.push_str(&format!("- Files reviewed: {}\n", summary.files_reviewed));
    body.push_str(&format!(
        "- Overall score: {:.1}/10\n",
        summary.overall_score
    ));

    if summary.total_comments == 0 {
        body.push_str("\nNo issues detected in this PR by DiffScope.\n");
        return body;
    }

    body.push_str("\n### Severity Breakdown\n");
    for severity in ["Error", "Warning", "Info", "Suggestion"] {
        let count = summary.by_severity.get(severity).copied().unwrap_or(0);
        body.push_str(&format!("- {}: {}\n", severity, count));
    }

    let rule_hits = summarize_rule_hits(comments, 8, rule_priority);
    if !rule_hits.is_empty() {
        body.push_str("\n### Rule Hits\n");
        for (rule_id, hit) in rule_hits {
            body.push_str(&format!(
                "- `{}`: {} hit(s) (E:{} W:{} I:{} S:{})\n",
                rule_id, hit.total, hit.errors, hit.warnings, hit.infos, hit.suggestions
            ));
        }
    }

    body.push_str("\n### Top Findings by File\n");
    body.push_str(&format_top_findings_by_file(comments, 5, 2));

    body
}

pub fn load_review_rules(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    repo_root: &std::path::Path,
) -> Vec<core::ReviewRule> {
    let mut rules = Vec::new();
    let local_patterns = if config.rules_files.is_empty() {
        vec![
            ".diffscope-rules.yml".to_string(),
            ".diffscope-rules.yaml".to_string(),
            ".diffscope-rules.json".to_string(),
            "rules/**/*.yml".to_string(),
            "rules/**/*.yaml".to_string(),
            "rules/**/*.json".to_string(),
        ]
    } else {
        config.rules_files.clone()
    };

    let local_max_rules = config.max_active_rules.saturating_mul(8).max(64);
    match core::load_rules_from_patterns(repo_root, &local_patterns, "repository", local_max_rules)
    {
        Ok(mut loaded) => rules.append(&mut loaded),
        Err(err) => warn!("Failed to load repository rules: {}", err),
    }

    for repo in &config.pattern_repositories {
        if repo.rule_patterns.is_empty() {
            continue;
        }
        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let max_rules = repo.max_rules.max(config.max_active_rules);
        match core::load_rules_from_patterns(
            base_path,
            &repo.rule_patterns,
            &repo.source,
            max_rules,
        ) {
            Ok(mut loaded) => rules.append(&mut loaded),
            Err(err) => warn!(
                "Failed to load pattern repository rules from '{}': {}",
                repo.source, err
            ),
        }
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for rule in rules {
        let key = rule.id.trim().to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        unique.push(rule);
    }

    if !unique.is_empty() {
        info!("Loaded {} review rule(s)", unique.len());
    }
    unique
}

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

    context_chunks.push(core::LLMContextChunk {
        content: lines.join("\n"),
        context_type: core::ContextType::Documentation,
        file_path: diff.file_path.clone(),
        line_range: None,
    });
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
    fn normalize_rule_id_trims_and_lowercases() {
        assert_eq!(
            normalize_rule_id(Some(" SEC.XSS ")),
            Some("sec.xss".to_string())
        );
        assert_eq!(normalize_rule_id(Some("")), None);
        assert_eq!(normalize_rule_id(None), None);
    }

    #[test]
    fn summarize_rule_hits_orders_by_volume() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        c3.rule_id = Some("rule.beta".to_string());

        let hits = summarize_rule_hits(&[c1, c2, c3], 8, &[]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "rule.alpha");
        assert_eq!(hits[0].1.total, 2);
        assert_eq!(hits[1].0, "rule.beta");
    }

    #[test]
    fn summarize_rule_hits_respects_priority_order() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.9,
        );
        c3.rule_id = Some("rule.beta".to_string());

        let hits = summarize_rule_hits(
            &[c1, c2, c3],
            8,
            &["rule.beta".to_string(), "rule.alpha".to_string()],
        );
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "rule.beta");
    }

    #[test]
    fn summarize_rule_hits_skips_comments_without_rules() {
        let c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        let hits = summarize_rule_hits(&[c1], 8, &[]);
        assert!(hits.is_empty());
    }

    #[test]
    fn top_findings_summary_groups_by_file() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.file_path = PathBuf::from("src/a.rs");
        c1.line_number = 11;
        c1.rule_id = Some("rule.alpha".to_string());

        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.file_path = PathBuf::from("src/a.rs");
        c2.line_number = 20;

        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        c3.file_path = PathBuf::from("src/b.rs");
        c3.line_number = 5;

        let text = format_top_findings_by_file(&[c1, c2, c3], 5, 2);
        assert!(text.contains("`src/a.rs` (2 issue(s))"));
        assert!(text.contains("`L11` [Error rule:rule.alpha]"));
        assert!(text.contains("`src/b.rs` (1 issue(s))"));
    }

    #[test]
    fn top_findings_empty_comments() {
        let text = format_top_findings_by_file(&[], 5, 2);
        assert_eq!(text, "- None\n");
    }

    #[test]
    fn build_rule_priority_rank_basic() {
        let rank = build_rule_priority_rank(&["sec.xss".to_string(), "sec.sqli".to_string()]);
        assert_eq!(rank.get("sec.xss"), Some(&0));
        assert_eq!(rank.get("sec.sqli"), Some(&1));
    }

    #[test]
    fn build_rule_priority_rank_deduplicates() {
        let rank = build_rule_priority_rank(&["SEC.XSS".to_string(), "sec.xss".to_string()]);
        assert_eq!(rank.len(), 1);
        assert_eq!(rank.get("sec.xss"), Some(&0));
    }

    #[test]
    fn severity_rank_order() {
        assert!(
            severity_rank(&core::comment::Severity::Error)
                < severity_rank(&core::comment::Severity::Warning)
        );
        assert!(
            severity_rank(&core::comment::Severity::Warning)
                < severity_rank(&core::comment::Severity::Info)
        );
        assert!(
            severity_rank(&core::comment::Severity::Info)
                < severity_rank(&core::comment::Severity::Suggestion)
        );
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

    #[test]
    fn pr_summary_body_includes_key_sections() {
        let mut c = build_comment(
            "c1",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.9,
        );
        c.rule_id = Some("sec.xss".to_string());
        let body = build_pr_summary_comment_body(&[c], &[]);
        assert!(body.contains("DiffScope Review Summary"));
        assert!(body.contains("Total issues: 1"));
        assert!(body.contains("sec.xss"));
    }

    #[test]
    fn pr_summary_body_empty_comments() {
        let body = build_pr_summary_comment_body(&[], &[]);
        assert!(body.contains("No issues detected"));
    }
}
