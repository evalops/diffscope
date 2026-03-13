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
    pattern.line.is_none_or(|line| comment.line_number == line)
}

pub(super) fn matches_contains(pattern: &EvalPattern, content_lower: &str) -> bool {
    if let Some(contains) = &pattern.contains {
        let needle = contains.trim().to_ascii_lowercase();
        if !needle.is_empty() {
            return content_lower.contains(&needle);
        }
    }

    true
}

pub(super) fn matches_contains_any(pattern: &EvalPattern, content_lower: &str) -> bool {
    let contains_any = pattern
        .contains_any
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    contains_any.is_empty()
        || contains_any
            .iter()
            .any(|needle| content_lower.contains(needle))
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
                .any(|tag| tag.eq_ignore_ascii_case(expected))
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
        comment
            .severity
            .to_string()
            .eq_ignore_ascii_case(severity.trim())
    })
}

pub(super) fn matches_category(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    pattern.category.as_ref().is_none_or(|category| {
        comment
            .category
            .to_string()
            .eq_ignore_ascii_case(category.trim())
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
