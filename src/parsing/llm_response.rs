use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

use crate::core;

pub fn parse_llm_response(content: &str, file_path: &Path) -> Result<Vec<core::comment::RawComment>> {
    let mut comments = Vec::new();
    static LINE_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)line\s+(\d+)((?:\s*(?:\[[^\]]+\]|\([^)]+\)))*)\s*:\s*(.+)").unwrap()
    });

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and common non-issue lines
        if trimmed.is_empty()
            || trimmed.starts_with("```")
            || trimmed.starts_with('#')
            || trimmed.starts_with('<')
            || trimmed.contains("Here are")
            || trimmed.contains("Here is")
            || trimmed.contains("review of")
        {
            continue;
        }

        if let Some(caps) = LINE_PATTERN.captures(line) {
            let line_number: usize = caps.get(1).unwrap().as_str().parse()?;
            let metadata = caps.get(2).map(|value| value.as_str()).unwrap_or("");
            let comment_text = caps.get(3).unwrap().as_str().trim();
            let (inline_rule_id, comment_text) = extract_rule_id_from_text(comment_text);
            let metadata_rule_id = extract_rule_id_from_metadata(metadata);
            let rule_id = inline_rule_id.or(metadata_rule_id);

            // Extract suggestion if present
            let (content, suggestion) = if let Some(sugg_idx) = comment_text.rfind(". Consider ") {
                (
                    comment_text[..sugg_idx + 1].to_string(),
                    Some(
                        comment_text[sugg_idx + 11..]
                            .trim_end_matches('.')
                            .to_string(),
                    ),
                )
            } else if let Some(sugg_idx) = comment_text.rfind(". Use ") {
                (
                    comment_text[..sugg_idx + 1].to_string(),
                    Some(
                        comment_text[sugg_idx + 6..]
                            .trim_end_matches('.')
                            .to_string(),
                    ),
                )
            } else {
                (comment_text.to_string(), None)
            };

            comments.push(core::comment::RawComment {
                file_path: file_path.to_path_buf(),
                line_number,
                content,
                rule_id,
                suggestion,
                severity: None,
                category: None,
                confidence: None,
                fix_effort: None,
                tags: Vec::new(),
            });
        }
    }

    Ok(comments)
}

pub fn extract_rule_id_from_text(text: &str) -> (Option<String>, String) {
    static BRACKET_RULE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\[\s*rule\s*:\s*([a-z0-9_.-]+)\s*\]").unwrap());
    static PREFIX_RULE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)^rule[\s:#-]+([a-z0-9_.-]+)\s*[-:]\s*(.+)$").unwrap());

    if let Some(caps) = BRACKET_RULE.captures(text) {
        let rule_id = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty());
        let stripped = BRACKET_RULE.replace(text, "").trim().to_string();
        return (rule_id, stripped);
    }

    if let Some(caps) = PREFIX_RULE.captures(text) {
        let rule_id = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty());
        let stripped = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| text.trim().to_string());
        return (rule_id, stripped);
    }

    (None, text.trim().to_string())
}

pub fn extract_rule_id_from_metadata(metadata: &str) -> Option<String> {
    static META_RULE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)rule\s*[:=]\s*([a-z0-9_.-]+)").unwrap());

    META_RULE
        .captures(metadata)
        .and_then(|captures| {
            captures
                .get(1)
                .map(|value| value.as_str().trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_llm_response_extracts_basic_comment() {
        let input = "Line 10: This is a basic issue.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 10);
        assert!(comments[0].content.contains("This is a basic issue"));
    }

    #[test]
    fn parse_llm_response_extracts_rule_from_line_metadata() {
        let input = "Line 12 [rule:sec.sql.injection]: Security - Raw SQL with user input.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 12);
        assert_eq!(comments[0].rule_id.as_deref(), Some("sec.sql.injection"));
    }

    #[test]
    fn parse_llm_response_extracts_suggestion_with_consider() {
        let input = "Line 5: Missing null check. Consider adding a guard clause.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].content, "Missing null check.");
        assert_eq!(
            comments[0].suggestion.as_deref(),
            Some("adding a guard clause")
        );
    }

    #[test]
    fn parse_llm_response_extracts_suggestion_with_use() {
        let input = "Line 8: Deprecated API call. Use the new v2 API instead.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].content, "Deprecated API call.");
        assert_eq!(
            comments[0].suggestion.as_deref(),
            Some("the new v2 API instead")
        );
    }

    #[test]
    fn parse_llm_response_skips_empty_and_noise_lines() {
        // The parser skips lines starting with ```, #, <, and noise phrases,
        // but does NOT track code-block state — content between ``` markers is still parsed.
        let input = "\n\n# Review\nHere are the issues:\n```\nLine 3: Real issue.\n```\n";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 3);

        // Verify noise-only input produces no comments
        let noise = "\n\n# Summary\nHere are the results:\n";
        let comments = parse_llm_response(noise, &file_path).unwrap();
        assert_eq!(comments.len(), 0);
    }

    #[test]
    fn parse_llm_response_handles_multiple_comments() {
        let input = "Line 1: First issue.\nLine 20: Second issue.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 1);
        assert_eq!(comments[1].line_number, 20);
    }

    #[test]
    fn extract_rule_id_from_text_bracket_syntax() {
        let (rule_id, text) = extract_rule_id_from_text("Something [rule: sec.auth] is wrong");
        assert_eq!(rule_id.as_deref(), Some("sec.auth"));
        assert_eq!(text, "Something  is wrong");
    }

    #[test]
    fn extract_rule_id_from_text_prefix_syntax() {
        let (rule_id, text) = extract_rule_id_from_text("rule: sec.auth - Missing auth check");
        assert_eq!(rule_id.as_deref(), Some("sec.auth"));
        assert_eq!(text, "Missing auth check");
    }

    #[test]
    fn extract_rule_id_from_text_no_rule() {
        let (rule_id, text) = extract_rule_id_from_text("Just a regular comment");
        assert!(rule_id.is_none());
        assert_eq!(text, "Just a regular comment");
    }

    #[test]
    fn extract_rule_id_from_metadata_finds_rule() {
        let rule = extract_rule_id_from_metadata(" [rule:sec.xss] (Warning)");
        assert_eq!(rule.as_deref(), Some("sec.xss"));
    }

    #[test]
    fn extract_rule_id_from_metadata_empty() {
        let rule = extract_rule_id_from_metadata("(Warning)");
        assert!(rule.is_none());
    }
}
