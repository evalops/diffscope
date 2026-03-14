#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

use crate::core;

pub fn parse_smart_review_response(
    content: &str,
    file_path: &Path,
) -> Result<Vec<core::comment::RawComment>> {
    let mut comments = Vec::new();
    let mut current_comment: Option<core::comment::RawComment> = None;
    let mut section: Option<SmartSection> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(title) = trimmed.strip_prefix("ISSUE:") {
            // Save previous comment if exists
            if let Some(comment) = current_comment.take() {
                comments.push(comment);
            }

            // Start new comment
            let title = title.trim();
            current_comment = Some(core::comment::RawComment {
                file_path: file_path.to_path_buf(),
                line_number: 1,
                content: title.to_string(),
                rule_id: None,
                suggestion: None,
                severity: None,
                category: None,
                confidence: None,
                fix_effort: None,
                tags: Vec::new(),
                code_suggestion: None,
            });
            section = None;
            continue;
        }

        let comment = match current_comment.as_mut() {
            Some(comment) => comment,
            None => continue,
        };

        if let Some(value) = trimmed.strip_prefix("LINE:") {
            if let Ok(line_num) = value.trim().parse::<usize>() {
                comment.line_number = line_num;
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("RULE:") {
            let value = value.trim();
            if value.is_empty() {
                comment.rule_id = None;
            } else {
                comment.rule_id = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("SEVERITY:") {
            comment.severity = parse_smart_severity(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("CATEGORY:") {
            comment.category = parse_smart_category(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("CONFIDENCE:") {
            comment.confidence = parse_smart_confidence(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("EFFORT:") {
            comment.fix_effort = parse_smart_effort(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("TAGS:") {
            comment.tags = parse_smart_tags(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("DESCRIPTION:") {
            section = Some(SmartSection::Description);
            let value = value.trim();
            if !value.is_empty() {
                append_content(&mut comment.content, value);
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("SUGGESTION:") {
            section = Some(SmartSection::Suggestion);
            let value = value.trim();
            if !value.is_empty() {
                append_suggestion(&mut comment.suggestion, value);
            }
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        match section {
            Some(SmartSection::Suggestion) => append_suggestion(&mut comment.suggestion, trimmed),
            _ => append_content(&mut comment.content, trimmed),
        }
    }

    // Save last comment
    if let Some(comment) = current_comment {
        comments.push(comment);
    }

    Ok(comments)
}

#[derive(Clone, Copy)]
enum SmartSection {
    Description,
    Suggestion,
}

fn append_content(content: &mut String, value: &str) {
    if !content.is_empty() {
        content.push(' ');
    }
    content.push_str(value);
}

fn append_suggestion(suggestion: &mut Option<String>, value: &str) {
    match suggestion {
        Some(existing) => {
            if !existing.is_empty() {
                existing.push(' ');
            }
            existing.push_str(value);
        }
        None => {
            *suggestion = Some(value.to_string());
        }
    }
}

pub fn parse_smart_severity(value: &str) -> Option<core::comment::Severity> {
    match value.to_lowercase().as_str() {
        "critical" | "error" => Some(core::comment::Severity::Error),
        "high" | "warning" => Some(core::comment::Severity::Warning),
        "medium" | "info" => Some(core::comment::Severity::Info),
        "low" | "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

pub fn parse_smart_category(value: &str) -> Option<core::comment::Category> {
    match value.to_lowercase().as_str() {
        "security" => Some(core::comment::Category::Security),
        "performance" => Some(core::comment::Category::Performance),
        "bug" => Some(core::comment::Category::Bug),
        "maintainability" => Some(core::comment::Category::Maintainability),
        "testing" => Some(core::comment::Category::Testing),
        "style" => Some(core::comment::Category::Style),
        "documentation" => Some(core::comment::Category::Documentation),
        "architecture" => Some(core::comment::Category::Architecture),
        "bestpractice" | "best_practice" | "best practice" => {
            Some(core::comment::Category::BestPractice)
        }
        _ => None,
    }
}

pub fn parse_smart_confidence(value: &str) -> Option<f32> {
    let raw = value.trim();
    let has_percent = raw.ends_with('%');
    let trimmed = raw.trim_end_matches('%');
    if let Ok(num) = trimmed.parse::<f32>() {
        if !has_percent && (0.0..=1.0).contains(&num) {
            // Bare decimal in 0..1 range — treat as already-normalized confidence
            Some(num.clamp(0.0, 1.0))
        } else {
            // Percentage value (e.g., "85" or "85%") — divide by 100
            Some((num / 100.0).clamp(0.0, 1.0))
        }
    } else {
        None
    }
}

pub fn parse_smart_effort(value: &str) -> Option<core::comment::FixEffort> {
    match value.to_lowercase().as_str() {
        "low" => Some(core::comment::FixEffort::Low),
        "medium" => Some(core::comment::FixEffort::Medium),
        "high" => Some(core::comment::FixEffort::High),
        _ => None,
    }
}

pub fn parse_smart_tags(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| tag.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_smart_review_response_parses_fields() {
        let input = r#"
ISSUE: Missing auth check
LINE: 42
RULE: sec.auth.guard
SEVERITY: CRITICAL
CATEGORY: Security
CONFIDENCE: 85%
EFFORT: High

DESCRIPTION:
Authentication is missing.

SUGGESTION:
Add a guard.

TAGS: auth, security
"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);

        let comment = &comments[0];
        assert_eq!(comment.line_number, 42);
        assert_eq!(comment.rule_id.as_deref(), Some("sec.auth.guard"));
        assert_eq!(comment.severity, Some(core::comment::Severity::Error));
        assert_eq!(comment.category, Some(core::comment::Category::Security));
        assert!(comment.content.contains("Missing auth check"));
        assert!(comment.content.contains("Authentication is missing."));
        assert_eq!(comment.suggestion.as_deref(), Some("Add a guard."));
        assert_eq!(
            comment.tags,
            vec!["auth".to_string(), "security".to_string()]
        );

        let confidence = comment.confidence.unwrap_or(0.0);
        assert!((confidence - 0.85).abs() < 0.0001);
        assert_eq!(comment.fix_effort, Some(core::comment::FixEffort::High));
    }

    #[test]
    fn parse_smart_review_response_handles_multiple_issues() {
        let input = "ISSUE: First\nLINE: 1\nISSUE: Second\nLINE: 2\n";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 1);
        assert!(comments[0].content.contains("First"));
        assert_eq!(comments[1].line_number, 2);
        assert!(comments[1].content.contains("Second"));
    }

    #[test]
    fn parse_smart_review_response_empty_rule_becomes_none() {
        let input = "ISSUE: Test\nRULE:\nLINE: 5\n";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].rule_id.is_none());
    }

    #[test]
    fn parse_smart_review_response_multiline_description() {
        let input = "ISSUE: Test\nDESCRIPTION:\nFirst line.\nSecond line.\n";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].content.contains("First line."));
        assert!(comments[0].content.contains("Second line."));
    }

    #[test]
    fn parse_smart_review_response_multiline_suggestion() {
        let input = "ISSUE: Test\nSUGGESTION:\nDo this.\nAlso that.\n";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        let suggestion = comments[0].suggestion.as_deref().unwrap();
        assert!(suggestion.contains("Do this."));
        assert!(suggestion.contains("Also that."));
    }

    #[test]
    fn parse_smart_severity_maps_correctly() {
        assert_eq!(
            parse_smart_severity("CRITICAL"),
            Some(core::comment::Severity::Error)
        );
        assert_eq!(
            parse_smart_severity("high"),
            Some(core::comment::Severity::Warning)
        );
        assert_eq!(
            parse_smart_severity("Medium"),
            Some(core::comment::Severity::Info)
        );
        assert_eq!(
            parse_smart_severity("low"),
            Some(core::comment::Severity::Suggestion)
        );
        assert_eq!(parse_smart_severity("unknown"), None);
    }

    #[test]
    fn parse_smart_category_maps_correctly() {
        assert_eq!(
            parse_smart_category("security"),
            Some(core::comment::Category::Security)
        );
        assert_eq!(
            parse_smart_category("Performance"),
            Some(core::comment::Category::Performance)
        );
        assert_eq!(
            parse_smart_category("BestPractice"),
            Some(core::comment::Category::BestPractice)
        );
        assert_eq!(
            parse_smart_category("best practice"),
            Some(core::comment::Category::BestPractice)
        );
        assert_eq!(parse_smart_category("unknown"), None);
    }

    #[test]
    fn parse_smart_confidence_handles_percent() {
        let conf = parse_smart_confidence("85%").unwrap();
        assert!((conf - 0.85).abs() < 0.001);
    }

    #[test]
    fn parse_smart_confidence_clamps_range() {
        assert_eq!(parse_smart_confidence("150%"), Some(1.0));
        assert_eq!(parse_smart_confidence("-10%"), Some(0.0));
    }

    #[test]
    fn parse_smart_confidence_invalid() {
        assert!(parse_smart_confidence("abc").is_none());
    }

    #[test]
    fn parse_smart_effort_maps_correctly() {
        assert_eq!(
            parse_smart_effort("low"),
            Some(core::comment::FixEffort::Low)
        );
        assert_eq!(
            parse_smart_effort("MEDIUM"),
            Some(core::comment::FixEffort::Medium)
        );
        assert_eq!(
            parse_smart_effort("High"),
            Some(core::comment::FixEffort::High)
        );
        assert_eq!(parse_smart_effort("nope"), None);
    }

    #[test]
    fn parse_smart_tags_splits_and_trims() {
        let tags = parse_smart_tags("auth, security , perf");
        assert_eq!(tags, vec!["auth", "security", "perf"]);
    }

    #[test]
    fn parse_smart_tags_empty_input() {
        let tags = parse_smart_tags("");
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_smart_tags_skips_empty_entries() {
        let tags = parse_smart_tags("auth,,, security");
        assert_eq!(tags, vec!["auth", "security"]);
    }

    // ── Bug: parse_smart_severity rejects standard severity names ────────
    //
    // The prompt instructs the LLM to use CRITICAL|HIGH|MEDIUM|LOW, but
    // parse_rule_severity_override (in rule_helpers.rs) accepts both name
    // families.  LLMs frequently output the canonical Severity enum names
    // (e.g. "SEVERITY: Error" or "SEVERITY: Warning") and those silently
    // become None, losing severity information.

    #[test]
    fn parse_smart_severity_accepts_standard_names() {
        // "error" / "warning" / "info" / "suggestion" are the enum names
        // used everywhere else in the codebase and are commonly output by
        // LLMs. parse_smart_severity should accept them.
        assert_eq!(
            parse_smart_severity("error"),
            Some(core::comment::Severity::Error),
            "\"error\" should map to Severity::Error"
        );
        assert_eq!(
            parse_smart_severity("warning"),
            Some(core::comment::Severity::Warning),
            "\"warning\" should map to Severity::Warning"
        );
        assert_eq!(
            parse_smart_severity("info"),
            Some(core::comment::Severity::Info),
            "\"info\" should map to Severity::Info"
        );
        assert_eq!(
            parse_smart_severity("suggestion"),
            Some(core::comment::Severity::Suggestion),
            "\"suggestion\" should map to Severity::Suggestion"
        );
    }

    // ── Bug: parse_smart_confidence misinterprets bare decimals ──────────
    //
    // When an LLM outputs "CONFIDENCE: 0.85" (a bare float instead of
    // "85%"), parse_smart_confidence divides by 100 producing 0.0085,
    // which effectively discards every comment at the default confidence
    // threshold.  Values <= 1.0 without a '%' suffix should be treated
    // as already-normalized (0.0..1.0).

    #[test]
    fn parse_smart_confidence_bare_decimal_treated_as_fraction() {
        // 0.85 is clearly a 0..1 confidence, not 0.85%
        let conf = parse_smart_confidence("0.85").unwrap();
        assert!(
            (conf - 0.85).abs() < 0.001,
            "0.85 should be treated as 85% confidence, got {conf}"
        );
    }

    #[test]
    fn parse_smart_confidence_bare_decimal_zero_point_five() {
        let conf = parse_smart_confidence("0.5").unwrap();
        assert!(
            (conf - 0.5).abs() < 0.001,
            "0.5 should be treated as 50% confidence, got {conf}"
        );
    }
}
