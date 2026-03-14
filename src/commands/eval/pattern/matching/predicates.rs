use regex::Regex;

use crate::core;

use super::super::super::EvalPattern;

impl EvalPattern {
    pub(super) fn is_empty(&self) -> bool {
        self.file.as_deref().map(str::trim).unwrap_or("").is_empty()
            && self.line.is_none()
            && self
                .contains
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .contains_any
                .iter()
                .all(|value| value.trim().is_empty())
            && self
                .matches_regex
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .severity
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .category
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self.tags_any.iter().all(|value| value.trim().is_empty())
            && self.confidence_at_least.is_none()
            && self.confidence_at_most.is_none()
            && self
                .fix_effort
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .rule_id_aliases
                .iter()
                .all(|value| value.trim().is_empty())
            && (!self.require_rule_id
                || self
                    .rule_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty())
    }
}

pub(super) fn matches_file(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    if let Some(file) = &pattern.file {
        let file = file.trim();
        if !file.is_empty() {
            let candidate = comment.file_path.to_string_lossy();
            return candidate == file || candidate.ends_with(file);
        }
    }

    true
}

pub(super) fn matches_line(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    pattern
        .line
        .is_none_or(|line| comment.line_number.abs_diff(line) <= 1)
}

pub(super) fn matches_contains(pattern: &EvalPattern, content_lower: &str) -> bool {
    if let Some(contains) = &pattern.contains {
        let needle = contains.trim();
        if !needle.is_empty() {
            return semantic_text_matches(content_lower, needle);
        }
    }

    true
}

pub(super) fn matches_contains_any(pattern: &EvalPattern, content_lower: &str) -> bool {
    let contains_any = pattern
        .contains_any
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    contains_any.is_empty()
        || contains_any
            .iter()
            .any(|needle| semantic_text_matches(content_lower, needle))
}

pub(super) fn matches_tags_any(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    let tags_any = pattern
        .tags_any
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    tags_any.is_empty()
        || tags_any.iter().any(|expected| {
            comment
                .tags
                .iter()
                .any(|tag| semantic_text_matches(tag, expected))
        })
}

pub(super) fn matches_regex(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    if let Some(regex_pattern) = pattern.matches_regex.as_deref().map(str::trim) {
        if !regex_pattern.is_empty() {
            return Regex::new(regex_pattern)
                .map(|regex| regex.is_match(&comment.content))
                .unwrap_or(false);
        }
    }

    true
}

pub(super) fn matches_severity(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    pattern.severity.as_ref().is_none_or(|severity| {
        let expected = severity.trim();
        comment.severity.to_string().eq_ignore_ascii_case(expected)
            || severity_rank(comment.severity.as_str()) >= severity_rank(expected)
    })
}

pub(super) fn matches_category(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    pattern.category.as_ref().is_none_or(|category| {
        let expected = category.trim();
        comment.category.to_string().eq_ignore_ascii_case(expected)
            || semantic_category_matches(expected, comment)
    })
}

pub(super) fn matches_confidence_bounds(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    pattern
        .confidence_at_least
        .is_none_or(|min_confidence| comment.confidence >= min_confidence)
        && pattern
            .confidence_at_most
            .is_none_or(|max_confidence| comment.confidence <= max_confidence)
}

pub(super) fn matches_fix_effort(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    if let Some(fix_effort) = &pattern.fix_effort {
        let expected = fix_effort.trim();
        if !expected.is_empty() {
            return format!("{:?}", comment.fix_effort).eq_ignore_ascii_case(expected);
        }
    }

    true
}

fn semantic_text_matches(content: &str, needle: &str) -> bool {
    let needle_lower = needle.trim().to_ascii_lowercase();
    if needle_lower.is_empty() {
        return true;
    }
    if content.contains(&needle_lower) {
        return true;
    }

    let canonical_content = canonicalize_semantic_text(content);
    let canonical_needle = canonicalize_semantic_text(&needle_lower);
    if canonical_content.contains(&canonical_needle) {
        return true;
    }

    let needle_tokens = semantic_tokens(&canonical_needle);
    if needle_tokens.is_empty() {
        return true;
    }
    let content_tokens = semantic_tokens(&canonical_content);
    needle_tokens
        .iter()
        .all(|token| content_tokens.iter().any(|candidate| candidate == token))
}

fn semantic_category_matches(expected: &str, comment: &core::Comment) -> bool {
    let expected = canonicalize_category(expected);
    if expected.is_empty() {
        return true;
    }
    if canonicalize_category(&comment.category.to_string()) == expected {
        return true;
    }

    let search_space = format!(
        "{} {}",
        comment.content.to_ascii_lowercase(),
        comment.tags.join(" ").to_ascii_lowercase()
    );
    category_aliases(&expected)
        .iter()
        .any(|alias| semantic_text_matches(&search_space, alias))
}

fn canonicalize_category(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn category_aliases(expected: &str) -> &'static [&'static str] {
    match expected {
        "security" => &[
            "security",
            "authorization",
            "authentication",
            "access control",
            "permission",
            "privilege escalation",
            "authorization bypass",
            "idor",
            "injection",
            "path traversal",
            "open redirect",
            "supply chain",
            "secret",
            "forbidden",
            "unauthorized",
        ],
        "bug" => &[
            "bug",
            "panic",
            "crash",
            "nil",
            "null",
            "fire and forget",
            "detached task",
            "background task",
            "spawned task",
            "not awaited",
            "missing await",
            "promise is always truthy",
            "swallowed error",
            "logic error",
            "race condition",
            "deadlock",
        ],
        "performance" => &[
            "performance",
            "slow",
            "latency",
            "n plus one",
            "query inside loop",
            "memory leak",
        ],
        "style" => &["style", "format", "naming", "lint"],
        "documentation" => &["documentation", "docstring", "docs"],
        "bestpractice" => &["best practice", "robustness", "guardrail"],
        "maintainability" => &[
            "maintainability",
            "readability",
            "duplication",
            "complexity",
            "refactor",
        ],
        "testing" => &["testing", "test coverage", "missing test"],
        "architecture" => &["architecture", "design", "abstraction", "coupling"],
        _ => &[],
    }
}

fn canonicalize_semantic_text(text: &str) -> String {
    let mut canonical = text.to_ascii_lowercase();
    for (source, replacement) in [
        ("authz", "authorization"),
        ("authorisation", "authorization"),
        ("access control", "authorization"),
        ("broken access control", "authorization bypass"),
        ("verbose-error", "information disclosure"),
        ("verbose error", "information disclosure"),
        ("debug-details", "information disclosure"),
        ("debug details", "information disclosure"),
        ("stack-trace", "information disclosure"),
        ("stack trace", "information disclosure"),
        ("cwe-209", "information disclosure"),
        ("cwe 209", "information disclosure"),
        ("piping curl output directly to bash", "curl pipe to shell"),
        ("pipe curl output directly to bash", "curl pipe to shell"),
        (
            "piping remote script directly to bash",
            "curl pipe to shell",
        ),
        ("piping a remote script to bash", "curl pipe to shell"),
        ("arbitrary shell command execution", "command injection"),
        (
            "without input validation or sanitization",
            "user controlled command",
        ),
        ("untrusted code", "remote script"),
        ("attack vector", "risk"),
        ("silently discarded", "swallowed error"),
        ("silent failure", "swallowed error"),
        ("sqli", "sql injection"),
        ("xss", "cross site scripting"),
        ("ssrf", "server side request forgery"),
        ("xxe", "xml external entity"),
        ("rce", "remote code execution"),
        ("n+1", "n plus one"),
        ("n plus 1", "n plus one"),
        ("directory traversal", "path traversal"),
        ("cross-file", "cross file"),
        ("use-after-free", "use after free"),
    ] {
        canonical = canonical.replace(source, replacement);
    }
    canonical = canonical
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>();
    canonical = canonical.split_whitespace().collect::<Vec<_>>().join(" ");
    for (source, replacement) in [
        ("auth bypass", "authorization bypass"),
        ("auth check", "authorization check"),
        ("role check", "authorization check"),
    ] {
        canonical = canonical.replace(source, replacement);
    }
    canonical
}

fn severity_rank(value: &str) -> usize {
    match canonicalize_category(value).as_str() {
        "error" => 3,
        "warning" => 2,
        "suggestion" => 1,
        "info" => 0,
        _ => 0,
    }
}

fn semantic_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| match token.to_ascii_lowercase().as_str() {
            "auth" | "authz" => "authorization".to_string(),
            "authn" => "authentication".to_string(),
            "access" => "authorization".to_string(),
            "piping" | "piped" | "pipes" => "pipe".to_string(),
            "bash" | "sh" => "shell".to_string(),
            "downloaded" | "downloading" | "downloads" => "download".to_string(),
            "execution" | "executes" | "executed" | "executing" => "execute".to_string(),
            "verification" | "verifies" | "verified" | "verifying" => "verify".to_string(),
            "risks" => "risk".to_string(),
            other => other.to_string(),
        })
        .collect()
}
