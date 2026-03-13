use regex::Regex;

use crate::core;
use crate::review::normalize_rule_id;

use super::EvalPattern;

impl EvalPattern {
    pub(super) fn matches(&self, comment: &core::Comment) -> bool {
        if self.is_empty() {
            return false;
        }

        let content_lower = comment.content.to_ascii_lowercase();

        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                let candidate = comment.file_path.to_string_lossy();
                if !(candidate == file || candidate.ends_with(file)) {
                    return false;
                }
            }
        }

        if let Some(line) = self.line {
            if comment.line_number != line {
                return false;
            }
        }

        if let Some(contains) = &self.contains {
            let needle = contains.trim().to_ascii_lowercase();
            if !needle.is_empty() && !content_lower.contains(&needle) {
                return false;
            }
        }

        let contains_any: Vec<String> = self
            .contains_any
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect();
        if !contains_any.is_empty()
            && !contains_any
                .iter()
                .any(|needle| content_lower.contains(needle))
        {
            return false;
        }

        let tags_any: Vec<&str> = self
            .tags_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !tags_any.is_empty()
            && !tags_any.iter().any(|expected| {
                comment
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(expected))
            })
        {
            return false;
        }

        if let Some(pattern) = self.matches_regex.as_deref().map(str::trim) {
            if !pattern.is_empty()
                && !Regex::new(pattern)
                    .map(|regex| regex.is_match(&comment.content))
                    .unwrap_or(false)
            {
                return false;
            }
        }

        if let Some(severity) = &self.severity {
            if !comment
                .severity
                .to_string()
                .eq_ignore_ascii_case(severity.trim())
            {
                return false;
            }
        }

        if let Some(category) = &self.category {
            if !comment
                .category
                .to_string()
                .eq_ignore_ascii_case(category.trim())
            {
                return false;
            }
        }

        if let Some(min_confidence) = self.confidence_at_least {
            if comment.confidence < min_confidence {
                return false;
            }
        }

        if let Some(max_confidence) = self.confidence_at_most {
            if comment.confidence > max_confidence {
                return false;
            }
        }

        if let Some(fix_effort) = &self.fix_effort {
            let expected = fix_effort.trim();
            if !expected.is_empty()
                && !format!("{:?}", comment.fix_effort).eq_ignore_ascii_case(expected)
            {
                return false;
            }
        }

        if let Some(rule_id) = &self.rule_id {
            if self.require_rule_id {
                let expected = rule_id.trim().to_ascii_lowercase();
                let actual = comment
                    .rule_id
                    .as_deref()
                    .map(|value| value.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                if expected != actual {
                    return false;
                }
            }
        }

        true
    }

    pub(super) fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                parts.push(format!("file={}", file));
            }
        }
        if let Some(line) = self.line {
            parts.push(format!("line={}", line));
        }
        if let Some(contains) = &self.contains {
            let contains = contains.trim();
            if !contains.is_empty() {
                parts.push(format!("contains='{}'", contains));
            }
        }
        let contains_any: Vec<&str> = self
            .contains_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !contains_any.is_empty() {
            parts.push(format!("contains_any={}", contains_any.join("|")));
        }
        if let Some(pattern) = self.matches_regex.as_deref().map(str::trim) {
            if !pattern.is_empty() {
                parts.push(format!("matches_regex='{}'", pattern));
            }
        }
        if let Some(severity) = &self.severity {
            let severity = severity.trim();
            if !severity.is_empty() {
                parts.push(format!("severity={}", severity));
            }
        }
        if let Some(category) = &self.category {
            let category = category.trim();
            if !category.is_empty() {
                parts.push(format!("category={}", category));
            }
        }
        let tags_any: Vec<&str> = self
            .tags_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !tags_any.is_empty() {
            parts.push(format!("tags_any={}", tags_any.join("|")));
        }
        if let Some(min_confidence) = self.confidence_at_least {
            parts.push(format!("confidence>={:.2}", min_confidence));
        }
        if let Some(max_confidence) = self.confidence_at_most {
            parts.push(format!("confidence<={:.2}", max_confidence));
        }
        if let Some(fix_effort) = &self.fix_effort {
            let fix_effort = fix_effort.trim();
            if !fix_effort.is_empty() {
                parts.push(format!("fix_effort={}", fix_effort));
            }
        }
        if let Some(rule_id) = &self.rule_id {
            let rule_id = rule_id.trim();
            if !rule_id.is_empty() {
                if self.require_rule_id {
                    parts.push(format!("rule_id={} (required)", rule_id));
                } else {
                    parts.push(format!("rule_id={} (label)", rule_id));
                }
            }
        }

        if parts.is_empty() {
            "empty-pattern".to_string()
        } else {
            parts.join(", ")
        }
    }

    fn is_empty(&self) -> bool {
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
            && (!self.require_rule_id
                || self
                    .rule_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty())
    }

    pub(super) fn normalized_rule_id(&self) -> Option<String> {
        normalize_rule_id(self.rule_id.as_deref())
    }
}

pub(super) fn summarize_for_eval(content: &str) -> String {
    let mut summary = content.trim().replace('\n', " ");
    if summary.len() > 120 {
        let mut end = 117;
        while end > 0 && !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
        summary.push_str("...");
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    #[test]
    fn test_summarize_for_eval_short() {
        let result = summarize_for_eval("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_summarize_for_eval_utf8_safety() {
        let content = format!("a{}{}", "€".repeat(39), "abc");
        let result = summarize_for_eval(&content);
        assert!(result.len() <= 120);
    }

    #[test]
    fn test_eval_pattern_matches_regex_tags_and_confidence() {
        let comment = core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "Calling panic!(user_input) here can crash the request path".to_string(),
            rule_id: Some("panic.user-input".to_string()),
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: Some("Return an error instead of panicking".to_string()),
            confidence: 0.91,
            code_suggestion: None,
            tags: vec!["reliability".to_string(), "panic".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
        };

        let pattern = EvalPattern {
            contains_any: vec!["panic".to_string(), "unwrap".to_string()],
            matches_regex: Some("panic!\\([^)]*user_input[^)]*\\)".to_string()),
            tags_any: vec!["security".to_string(), "reliability".to_string()],
            confidence_at_least: Some(0.9),
            fix_effort: Some("low".to_string()),
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }
}
