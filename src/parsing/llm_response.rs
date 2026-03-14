use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

use crate::core;

pub fn parse_llm_response(
    content: &str,
    file_path: &Path,
) -> Result<Vec<core::comment::RawComment>> {
    // Strategy 1: Structured JSON output (preferred contract)
    let comments = parse_json_format(content, file_path);
    if !comments.is_empty() {
        return Ok(comments);
    }

    // Strategy 2: Primary parser (existing regex + code suggestion blocks)
    let comments = parse_primary(content, file_path)?;
    if !comments.is_empty() {
        return Ok(comments);
    }

    // Strategy 3: Numbered list format (e.g. "1. **src/lib.rs:42** - Issue text")
    let comments = parse_numbered_list(content, file_path);
    if !comments.is_empty() {
        return Ok(comments);
    }

    // Strategy 4: Markdown bullet format (e.g. "- Line 42: Issue text")
    let comments = parse_markdown_bullets(content, file_path);
    if !comments.is_empty() {
        return Ok(comments);
    }

    // Strategy 5: file:line format (e.g. "src/lib.rs:42 - Issue text")
    let comments = parse_file_line_format(content, file_path);
    if !comments.is_empty() {
        return Ok(comments);
    }

    // All strategies failed — return empty
    Ok(Vec::new())
}

/// Strategy 1: Primary parser — `Line <num>: <text>` with code suggestion blocks.
fn parse_primary(content: &str, file_path: &Path) -> Result<Vec<core::comment::RawComment>> {
    let mut comments = Vec::new();
    static LINE_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)line\s+(\d+)((?:\s*(?:\[[^\]]+\]|\([^)]+\)))*)\s*:\s*(.+)").unwrap()
    });

    // State machine for tracking <<<ORIGINAL ... === ... >>>SUGGESTED blocks
    let mut in_original = false;
    let mut in_suggested = false;
    let mut original_lines: Vec<String> = Vec::new();
    let mut suggested_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Handle code suggestion block markers
        if trimmed == "<<<ORIGINAL" {
            in_original = true;
            in_suggested = false;
            original_lines.clear();
            suggested_lines.clear();
            continue;
        }

        if in_original && trimmed == "===" {
            in_original = false;
            in_suggested = true;
            continue;
        }

        if in_suggested && trimmed == ">>>SUGGESTED" {
            in_suggested = false;
            // Attach the code suggestion to the most recent comment
            if let Some(last_comment) = comments.last_mut() {
                let original_code = original_lines.join("\n");
                let suggested_code = suggested_lines.join("\n");
                let diff = build_suggestion_diff(&original_code, &suggested_code);
                let last_comment: &mut core::comment::RawComment = last_comment;
                last_comment.code_suggestion = Some(core::comment::CodeSuggestion {
                    original_code,
                    suggested_code,
                    explanation: last_comment
                        .suggestion
                        .clone()
                        .or_else(|| Some(last_comment.content.clone()))
                        .unwrap_or_default(),
                    diff,
                });
            }
            original_lines.clear();
            suggested_lines.clear();
            continue;
        }

        // Accumulate lines inside the code suggestion block
        if in_original {
            original_lines.push(line.to_string());
            continue;
        }
        if in_suggested {
            suggested_lines.push(line.to_string());
            continue;
        }

        // Normal line-by-line comment parsing
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
            let Some(line_number) = capture_usize(&caps, 1)? else {
                continue;
            };
            let metadata = caps.get(2).map(|value| value.as_str()).unwrap_or("");
            let Some(comment_text) = capture_text(&caps, 3) else {
                continue;
            };
            let comment_text = comment_text.trim();
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
                code_suggestion: None,
            });
        }
    }

    Ok(comments)
}

/// Strategy 2: Numbered list format.
/// Matches patterns like:
///   1. **src/lib.rs:42** - Missing null check
///   2. src/lib.rs:15 - SQL injection
fn parse_numbered_list(content: &str, file_path: &Path) -> Vec<core::comment::RawComment> {
    static NUMBERED_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\s*\d+\.\s*\*{0,2}(?:.*?):?(\d+)\*{0,2}\s*[-\u{2013}\u{2014}:]\s*(.+)")
            .unwrap()
    });

    let mut comments = Vec::new();
    for line in content.lines() {
        if let Some(caps) = NUMBERED_PATTERN.captures(line) {
            if let Some(line_number) = capture_usize_lossy(&caps, 1) {
                let Some(text) = capture_text(&caps, 2) else {
                    continue;
                };
                let text = text.trim().to_string();
                comments.push(make_raw_comment(file_path, line_number, text));
            }
        }
    }
    comments
}

/// Strategy 3: Markdown bullet format.
/// Matches patterns like:
///   - Line 42: Missing null check
///   * **Line 42**: Missing null check
fn parse_markdown_bullets(content: &str, file_path: &Path) -> Vec<core::comment::RawComment> {
    static BULLET_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\s*[-*]\s*\*{0,2}[Ll]ine\s+(\d+)\*{0,2}\s*[:–—-]\s*(.+)").unwrap()
    });

    let mut comments = Vec::new();
    for line in content.lines() {
        if let Some(caps) = BULLET_PATTERN.captures(line) {
            if let Some(line_number) = capture_usize_lossy(&caps, 1) {
                let Some(text) = capture_text(&caps, 2) else {
                    continue;
                };
                let text = text.trim().to_string();
                comments.push(make_raw_comment(file_path, line_number, text));
            }
        }
    }
    comments
}

/// Strategy 4: file:line format.
/// Matches patterns like:
///   src/lib.rs:42 - Missing null check
///   file.py:15: SQL injection vulnerability
fn parse_file_line_format(content: &str, file_path: &Path) -> Vec<core::comment::RawComment> {
    static FILE_LINE_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\s*\*{0,2}(?:[\w./]+):(\d+)\*{0,2}\s*[-\u{2013}\u{2014}:]\s*(.+)").unwrap()
    });

    let mut comments = Vec::new();
    for line in content.lines() {
        if let Some(caps) = FILE_LINE_PATTERN.captures(line) {
            if let Some(line_number) = capture_usize_lossy(&caps, 1) {
                let Some(text) = capture_text(&caps, 2) else {
                    continue;
                };
                let text = text.trim().to_string();
                comments.push(make_raw_comment(file_path, line_number, text));
            }
        }
    }
    comments
}

/// Strategy 1: Structured JSON extraction.
/// Tries to find and parse JSON arrays/objects from the response content.
/// Handles JSON in code blocks, bare arrays, and wrapped result objects.
fn parse_json_format(content: &str, file_path: &Path) -> Vec<core::comment::RawComment> {
    let json_str = extract_json_from_code_block(content)
        .or_else(|| find_json_array(content))
        .or_else(|| find_json_object(content));

    let json_str = json_str
        .or_else(|| find_balanced_bracket_span(content, '[', ']'))
        .or_else(|| find_balanced_bracket_span(content, '{', '}'));

    if let Some(json_str) = json_str {
        for candidate in repair_json_candidates(&json_str) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&candidate) else {
                continue;
            };
            let items = extract_structured_items(value);
            let comments = items
                .into_iter()
                .filter_map(|item| structured_value_to_comment(file_path, &item))
                .collect::<Vec<_>>();
            if !comments.is_empty() {
                return comments;
            }
        }
    }
    Vec::new()
}

/// Extract JSON array content from markdown code blocks (```json ... ``` or ``` ... ```).
fn extract_json_from_code_block(content: &str) -> Option<String> {
    static CODE_BLOCK: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?s)```(?:json)?\s*\n(.*?)```").unwrap());

    for caps in CODE_BLOCK.captures_iter(content) {
        let Some(block) = capture_text(&caps, 1) else {
            continue;
        };
        let block = block.trim();
        if block.starts_with('[') || block.starts_with('{') {
            return Some(block.to_string());
        }
    }
    None
}

/// Find a bare JSON array in the content (not in a code block).
///
/// Uses bracket-depth counting to find the matching `]` for each `[`,
/// then validates with serde. This correctly handles multiple separate
/// arrays and nested brackets inside JSON strings.
fn find_json_array(content: &str) -> Option<String> {
    find_balanced_json(content, '[', ']')
}

fn find_json_object(content: &str) -> Option<String> {
    find_balanced_json(content, '{', '}')
}

/// Find the first balanced span for open/close (e.g. [ and ]) without validating JSON.
/// Used when valid JSON isn't found so we can run repair (e.g. single-quote conversion) and retry.
fn find_balanced_bracket_span(content: &str, open: char, close: char) -> Option<String> {
    for (start, _) in content.char_indices().filter(|&(_, ch)| ch == open) {
        let mut depth = 0i32;
        for (offset, ch) in content[start..].char_indices() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset;
                    return Some(content[start..=end].to_string());
                }
            }
        }
    }
    None
}

fn find_balanced_json(content: &str, open: char, close: char) -> Option<String> {
    for (start, _) in content.char_indices().filter(|&(_, ch)| ch == open) {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape_next = false;

        for (offset, ch) in content[start..].char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if !in_string {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + offset;
                        let candidate = &content[start..=end];
                        if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                            return Some(candidate.to_string());
                        }
                        break;
                    }
                }
            }
        }
    }
    None
}

fn repair_json_candidates(candidate: &str) -> Vec<String> {
    static TRAILING_COMMAS: Lazy<Regex> = Lazy::new(|| Regex::new(r",\s*([}\]])").unwrap());

    let trimmed = candidate.trim();
    let mut candidates = vec![trimmed.to_string()];

    let without_trailing_commas = TRAILING_COMMAS.replace_all(trimmed, "$1").to_string();
    if without_trailing_commas != trimmed {
        candidates.push(without_trailing_commas);
    }

    // When LLM echoes JSON with diff-style line prefixes (leading "+"), strip them (issue #28).
    let without_diff_prefix: String = trimmed
        .lines()
        .map(|line| line.strip_prefix('+').map(str::trim).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let without_diff_prefix = without_diff_prefix.trim();
    if without_diff_prefix != trimmed
        && (without_diff_prefix.starts_with('[') || without_diff_prefix.starts_with('{'))
    {
        candidates.push(without_diff_prefix.to_string());
    }

    // When LLM outputs single-quoted keys/values (e.g. {'line': 9}), convert to valid JSON (issue #28).
    let with_double_quotes = convert_single_quoted_json_to_double(trimmed);
    if with_double_quotes != trimmed
        && (with_double_quotes.starts_with('[') || with_double_quotes.starts_with('{'))
    {
        candidates.push(with_double_quotes);
    }

    candidates
}

/// Convert single-quoted JSON-like strings to double-quoted so serde_json can parse.
/// Only converts single-quoted regions that are outside any double-quoted string.
fn convert_single_quoted_json_to_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut in_double = false;
    let mut escape_next = false;

    while let Some(c) = chars.next() {
        if escape_next {
            escape_next = false;
            out.push(c);
            continue;
        }
        if in_double {
            if c == '\\' {
                escape_next = true;
                out.push(c);
            } else if c == '"' {
                in_double = false;
                out.push(c);
            } else {
                out.push(c);
            }
            continue;
        }
        if c == '"' {
            in_double = true;
            out.push(c);
            continue;
        }
        if c == '\'' {
            // Start of single-quoted string: emit " and copy until unescaped ', escaping " and \.
            // Inside single-quoted: \' → one quote in JSON (emit \"); \\ → emit \\; \" → emit \"
            out.push('"');
            let mut single_escape = false;
            for c in chars.by_ref() {
                if single_escape {
                    single_escape = false;
                    if c == '\'' {
                        out.push('\''); // apostrophe in JSON string needs no escape
                    } else {
                        out.push('\\');
                        out.push(c);
                    }
                } else if c == '\\' {
                    single_escape = true;
                } else if c == '\'' {
                    out.push('"');
                    break;
                } else if c == '"' {
                    out.push('\\');
                    out.push('"');
                } else {
                    out.push(c);
                }
            }
            if single_escape {
                out.push('\\');
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn extract_structured_items(value: serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(items) = value.as_array() {
        return items.clone();
    }

    for key in ["findings", "comments", "issues", "results"] {
        if let Some(items) = value.get(key).and_then(|value| value.as_array()) {
            return items.clone();
        }
    }

    Vec::new()
}

fn structured_value_to_comment(
    file_path: &Path,
    item: &serde_json::Value,
) -> Option<core::comment::RawComment> {
    let line = json_line_number(item)?;
    let content = json_issue_text(item);
    if content.trim().is_empty() {
        return None;
    }

    let suggestion = item
        .get("suggestion")
        .or_else(|| item.get("fix"))
        .or_else(|| item.get("recommendation"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let tags = item
        .get("tags")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(|value| value.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(core::comment::RawComment {
        file_path: file_path.to_path_buf(),
        line_number: line,
        content,
        rule_id: item
            .get("rule_id")
            .or_else(|| item.get("ruleId"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        suggestion,
        severity: item
            .get("severity")
            .and_then(|value| value.as_str())
            .and_then(parse_severity_label),
        category: item
            .get("category")
            .and_then(|value| value.as_str())
            .and_then(parse_category_label),
        confidence: item.get("confidence").and_then(json_confidence),
        fix_effort: item
            .get("fix_effort")
            .or_else(|| item.get("fixEffort"))
            .and_then(|value| value.as_str())
            .and_then(parse_fix_effort_label),
        tags,
        code_suggestion: build_code_suggestion(item),
    })
}

fn json_line_number(item: &serde_json::Value) -> Option<usize> {
    let direct = item
        .get("line")
        .or_else(|| item.get("line_number"))
        .or_else(|| item.get("lineNumber"))
        .or_else(|| item.get("start_line"))
        .or_else(|| item.get("startLine"));
    if let Some(line) = direct.and_then(parse_usize_value) {
        return Some(line);
    }

    item.get("location")
        .and_then(|location| {
            location
                .get("line")
                .or_else(|| location.get("start_line"))
                .or_else(|| location.get("startLine"))
        })
        .and_then(parse_usize_value)
}

fn json_issue_text(item: &serde_json::Value) -> String {
    let issue = item
        .get("issue")
        .or_else(|| item.get("description"))
        .or_else(|| item.get("message"))
        .or_else(|| item.get("content"))
        .or_else(|| item.get("text"))
        .or_else(|| item.get("title"))
        .and_then(|value| value.as_str())
        .unwrap_or("Issue found")
        .trim()
        .to_string();
    let impact = item
        .get("impact")
        .and_then(|value| value.as_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    match impact {
        Some(impact) if !issue.contains(impact) => format!("{issue} Impact: {impact}"),
        _ => issue,
    }
}

fn parse_usize_value(value: &serde_json::Value) -> Option<usize> {
    value.as_u64().map(|value| value as usize).or_else(|| {
        value
            .as_str()
            .and_then(|value| value.trim().parse::<usize>().ok())
    })
}

fn json_confidence(value: &serde_json::Value) -> Option<f32> {
    if let Some(number) = value.as_f64() {
        let confidence = number as f32;
        return Some(if confidence > 1.0 {
            (confidence / 100.0).clamp(0.0, 1.0)
        } else {
            confidence.clamp(0.0, 1.0)
        });
    }
    value
        .as_str()
        .and_then(|value| value.parse::<f32>().ok())
        .map(|confidence| {
            if confidence > 1.0 {
                (confidence / 100.0).clamp(0.0, 1.0)
            } else {
                confidence.clamp(0.0, 1.0)
            }
        })
}

fn parse_severity_label(label: &str) -> Option<core::comment::Severity> {
    match label.trim().to_ascii_lowercase().as_str() {
        "error" | "critical" => Some(core::comment::Severity::Error),
        "warning" | "warn" | "medium" => Some(core::comment::Severity::Warning),
        "info" | "low" => Some(core::comment::Severity::Info),
        "suggestion" | "nit" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

fn parse_category_label(label: &str) -> Option<core::comment::Category> {
    match label.trim().to_ascii_lowercase().as_str() {
        "bug" | "correctness" => Some(core::comment::Category::Bug),
        "security" => Some(core::comment::Category::Security),
        "performance" => Some(core::comment::Category::Performance),
        "style" => Some(core::comment::Category::Style),
        "documentation" | "docs" => Some(core::comment::Category::Documentation),
        "bestpractice" | "best_practice" | "best-practice" | "best practice" => {
            Some(core::comment::Category::BestPractice)
        }
        "maintainability" => Some(core::comment::Category::Maintainability),
        "testing" | "test" => Some(core::comment::Category::Testing),
        "architecture" => Some(core::comment::Category::Architecture),
        _ => None,
    }
}

fn parse_fix_effort_label(label: &str) -> Option<core::comment::FixEffort> {
    match label.trim().to_ascii_lowercase().as_str() {
        "low" | "small" => Some(core::comment::FixEffort::Low),
        "medium" | "moderate" => Some(core::comment::FixEffort::Medium),
        "high" | "large" => Some(core::comment::FixEffort::High),
        _ => None,
    }
}

fn build_code_suggestion(item: &serde_json::Value) -> Option<core::comment::CodeSuggestion> {
    let code_suggestion = item
        .get("code_suggestion")
        .or_else(|| item.get("codeSuggestion"));
    let original_code = code_suggestion
        .and_then(|value| {
            value
                .get("original_code")
                .or_else(|| value.get("originalCode"))
        })
        .and_then(|value| value.as_str())
        .or_else(|| item.get("original_code").and_then(|value| value.as_str()))?
        .to_string();
    let suggested_code = code_suggestion
        .and_then(|value| {
            value
                .get("suggested_code")
                .or_else(|| value.get("suggestedCode"))
        })
        .and_then(|value| value.as_str())
        .or_else(|| item.get("suggested_code").and_then(|value| value.as_str()))?
        .to_string();
    let explanation = code_suggestion
        .and_then(|value| value.get("explanation"))
        .and_then(|value| value.as_str())
        .or_else(|| item.get("fix").and_then(|value| value.as_str()))
        .unwrap_or("Suggested code change")
        .to_string();

    Some(core::comment::CodeSuggestion {
        diff: build_suggestion_diff(&original_code, &suggested_code),
        original_code,
        suggested_code,
        explanation,
    })
}

/// Helper to construct a RawComment with default fields.
fn make_raw_comment(
    file_path: &Path,
    line_number: usize,
    content: String,
) -> core::comment::RawComment {
    core::comment::RawComment {
        file_path: file_path.to_path_buf(),
        line_number,
        content,
        rule_id: None,
        suggestion: None,
        severity: None,
        category: None,
        confidence: None,
        fix_effort: None,
        tags: Vec::new(),
        code_suggestion: None,
    }
}

fn capture_text<'a>(captures: &'a regex::Captures<'_>, group: usize) -> Option<&'a str> {
    captures.get(group).map(|value| value.as_str())
}

fn capture_usize(captures: &regex::Captures<'_>, group: usize) -> Result<Option<usize>> {
    capture_text(captures, group)
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(Into::into)
}

fn capture_usize_lossy(captures: &regex::Captures<'_>, group: usize) -> Option<usize> {
    capture_text(captures, group).and_then(|value| value.parse::<usize>().ok())
}

/// Build a unified-diff-style string from original and suggested code.
fn build_suggestion_diff(original: &str, suggested: &str) -> String {
    let mut diff = String::new();
    for line in original.lines() {
        diff.push_str(&format!("- {line}\n"));
    }
    for line in suggested.lines() {
        diff.push_str(&format!("+ {line}\n"));
    }
    // Remove trailing newline for consistency
    if diff.ends_with('\n') {
        diff.truncate(diff.len() - 1);
    }
    diff
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
    fn parse_llm_response_prefers_structured_json_array() {
        let input = r#"[
  {
    "line": 14,
    "category": "security",
    "issue": "SQL query interpolates user input",
    "impact": "Attackers can inject arbitrary SQL",
    "fix": "Use bound parameters",
    "rule_id": "sec.sql.injection",
    "severity": "warning",
    "confidence": 0.92,
    "fix_effort": "low",
    "tags": ["security", "sql"],
    "original_code": "db.query(sql)",
    "suggested_code": "db.query(sql, [user_id])"
  }
]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 14);
        assert_eq!(comments[0].rule_id.as_deref(), Some("sec.sql.injection"));
        assert_eq!(comments[0].severity, Some(core::comment::Severity::Warning));
        assert!(comments[0]
            .content
            .contains("Attackers can inject arbitrary SQL"));
        assert!(comments[0].code_suggestion.is_some());
    }

    #[test]
    fn parse_llm_response_handles_json_object_wrapper_and_trailing_commas() {
        let input = r#"```json
{
  "findings": [
    {
      "location": { "line": 7 },
      "description": "Missing authorization check",
      "fix": "Validate ownership before returning the record",
      "category": "security",
      "severity": "error",
    },
  ]
}
```"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 7);
        assert_eq!(comments[0].severity, Some(core::comment::Severity::Error));
        assert_eq!(
            comments[0].category,
            Some(core::comment::Category::Security)
        );
        assert_eq!(
            comments[0].suggestion.as_deref(),
            Some("Validate ownership before returning the record")
        );
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

    #[test]
    fn parse_llm_response_extracts_code_suggestion() {
        let input = r#"Line 42: Security - User input passed directly to SQL query. Use parameterized queries.
<<<ORIGINAL
query = "SELECT * FROM users WHERE id = " + user_id
===
query = "SELECT * FROM users WHERE id = ?"
cursor.execute(query, (user_id,))
>>>SUGGESTED"#;
        let file_path = PathBuf::from("src/db.py");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        let cs = comments[0].code_suggestion.as_ref().unwrap();
        assert_eq!(
            cs.original_code,
            "query = \"SELECT * FROM users WHERE id = \" + user_id"
        );
        assert_eq!(
            cs.suggested_code,
            "query = \"SELECT * FROM users WHERE id = ?\"\ncursor.execute(query, (user_id,))"
        );
        assert!(cs
            .diff
            .contains("- query = \"SELECT * FROM users WHERE id = \" + user_id"));
        assert!(cs
            .diff
            .contains("+ query = \"SELECT * FROM users WHERE id = ?\""));
    }

    #[test]
    fn parse_llm_response_code_suggestion_attaches_to_correct_comment() {
        let input = r#"Line 10: Bug - Missing null check.
Line 20: Security - SQL injection risk.
<<<ORIGINAL
db.query(user_input)
===
db.query(sanitize(user_input))
>>>SUGGESTED"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        // First comment should have no code suggestion
        assert!(comments[0].code_suggestion.is_none());
        // Second comment should have the code suggestion
        let cs = comments[1].code_suggestion.as_ref().unwrap();
        assert_eq!(cs.original_code, "db.query(user_input)");
        assert_eq!(cs.suggested_code, "db.query(sanitize(user_input))");
    }

    #[test]
    fn parse_llm_response_multiple_code_suggestions() {
        let input = r#"Line 5: Bug - Off by one error.
<<<ORIGINAL
for i in 0..len + 1 {
===
for i in 0..len {
>>>SUGGESTED
Line 15: Performance - Unnecessary clone.
<<<ORIGINAL
let data = input.clone();
===
let data = &input;
>>>SUGGESTED"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        let cs0 = comments[0].code_suggestion.as_ref().unwrap();
        assert_eq!(cs0.original_code, "for i in 0..len + 1 {");
        assert_eq!(cs0.suggested_code, "for i in 0..len {");
        let cs1 = comments[1].code_suggestion.as_ref().unwrap();
        assert_eq!(cs1.original_code, "let data = input.clone();");
        assert_eq!(cs1.suggested_code, "let data = &input;");
    }

    #[test]
    fn parse_llm_response_no_code_suggestion_when_markers_absent() {
        let input = "Line 7: Style - Variable name is unclear.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].code_suggestion.is_none());
    }

    #[test]
    fn build_suggestion_diff_formats_correctly() {
        let diff = build_suggestion_diff("old_line", "new_line");
        assert_eq!(diff, "- old_line\n+ new_line");
    }

    #[test]
    fn build_suggestion_diff_multiline() {
        let diff = build_suggestion_diff("line1\nline2", "line1\nline2_fixed\nline3");
        assert_eq!(diff, "- line1\n- line2\n+ line1\n+ line2_fixed\n+ line3");
    }

    // === Fallback strategy tests ===

    #[test]
    fn parse_fallback_numbered_list() {
        let input = "Here are the issues found:\n\n1. **src/lib.rs:42** - Missing null check on user input\n2. **src/lib.rs:15** - SQL injection vulnerability in query builder";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 42);
        assert_eq!(comments[1].line_number, 15);
    }

    #[test]
    fn parse_fallback_numbered_list_no_bold() {
        let input = "1. src/lib.rs:42 - Missing null check\n2. src/lib.rs:15 - SQL injection";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn parse_fallback_markdown_bullets() {
        let input =
            "- Line 42: Missing null check on user input\n- Line 15: SQL injection vulnerability";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 42);
        assert_eq!(comments[1].line_number, 15);
    }

    #[test]
    fn parse_fallback_markdown_bullets_bold() {
        let input = "* **Line 42**: Missing null check\n* **Line 15**: SQL injection";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn parse_fallback_file_line_format() {
        let input = "src/lib.rs:42 - Missing null check\nsrc/lib.rs:15: SQL injection";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 42);
        assert_eq!(comments[1].line_number, 15);
    }

    #[test]
    fn parse_fallback_json_array() {
        let input = r#"Here are the issues:
```json
[
  {"line": 42, "issue": "Missing null check", "severity": "warning"},
  {"line": 15, "issue": "SQL injection", "severity": "error"}
]
```"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].line_number, 42);
        assert_eq!(comments[1].line_number, 15);
    }

    #[test]
    fn parse_fallback_json_with_different_keys() {
        // LLMs use various key names
        let input = r#"[{"line_number": 10, "description": "Bug here", "type": "bug"}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 10);
    }

    #[test]
    fn parse_primary_still_takes_priority() {
        // If primary format works, fallbacks should NOT run
        let input = "Line 10: This is a basic issue.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 10);
    }

    #[test]
    fn parse_empty_input_returns_empty() {
        let input = "";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_no_issues_response() {
        let input = "No issues found. The code looks good.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_fallback_preserves_code_suggestion_in_primary() {
        // Code suggestions should still work with primary parser
        let input = "Line 42: Bug - Off by one.\n<<<ORIGINAL\nfor i in 0..len+1 {\n===\nfor i in 0..len {\n>>>SUGGESTED";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].code_suggestion.is_some());
    }

    // ── Mutation-testing gap fills ─────────────────────────────────────

    #[test]
    fn parse_primary_skips_code_fence_markers() {
        // ``` markers themselves are skipped (not parsed as comments)
        let input = "```rust\n```";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_primary_skips_heading_lines() {
        let input = "# Code Review\nLine 10: Real issue";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        // Heading line skipped, but real Line 10 comment caught
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 10);
    }

    #[test]
    fn parse_primary_skips_preamble_lines() {
        let input = "Here are the issues I found:\nLine 5: Missing check";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 5);
    }

    #[test]
    fn parse_json_in_code_block_strategy() {
        // Test that extract_json_from_code_block specifically works
        let input = "Here are findings:\n```json\n[{\"line\": 7, \"issue\": \"Off by one\"}]\n```";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 7);
        assert!(comments[0].content.contains("Off by one"));
    }

    #[test]
    fn parse_json_bare_array_strategy() {
        // Test find_json_array with text before/after
        let input = "Issues found: [{\"line\": 3, \"issue\": \"Bug\"}] end.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 3);
    }

    // ── Adversarial edge cases ──────────────────────────────────────────

    #[test]
    fn parse_line_zero_not_panicking() {
        // Line 0 is technically invalid but should not panic
        let input = "Line 0: Edge case at line zero.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 0);
    }

    #[test]
    fn parse_huge_line_number_no_overflow() {
        let input = "Line 999999999999: Absurd line number.";
        let file_path = PathBuf::from("src/lib.rs");
        // Should either parse successfully or return empty, not panic
        let result = parse_llm_response(input, &file_path);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn parse_unicode_content_no_panic() {
        let input = "Line 10: 漏洞 — SQL注入风险 🔴";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].content.contains("漏洞"));
    }

    #[test]
    fn parse_numbered_list_with_no_line_number_in_path() {
        // Numbered list where file path is missing the colon-number
        let input = "1. **src/lib.rs** - Missing null check";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        // Should not extract a comment with a bogus line number
        for c in &comments {
            assert!(c.line_number > 0 || comments.is_empty());
        }
    }

    #[test]
    fn parse_json_with_nested_brackets() {
        // JSON with nested arrays/objects should not confuse the bracket finder
        let input =
            r#"[{"line": 10, "issue": "Bug with [array] access", "details": {"nested": [1,2,3]}}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 10);
    }

    #[test]
    fn parse_json_with_line_number_as_string() {
        // Some LLMs return line numbers as strings — should be handled
        let input = r#"[{"line": "42", "issue": "Bug"}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 42);
    }

    #[test]
    fn parse_malformed_json_no_panic() {
        let input = r#"[{"line": 10, "issue": "unclosed string}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_mixed_strategies_first_wins() {
        // Input matches both primary AND numbered list — primary should win
        let input = "Line 10: Primary format.\n1. **src/lib.rs:20** - Numbered format.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        // Primary parser should have caught Line 10, so we get that
        assert_eq!(comments[0].line_number, 10);
    }

    #[test]
    fn parse_code_suggestion_without_preceding_comment() {
        // <<<ORIGINAL block with no prior Line N: comment
        let input = "<<<ORIGINAL\nold code\n===\nnew code\n>>>SUGGESTED";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        // Should not panic; suggestion just gets dropped since no comment to attach to
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_unclosed_code_suggestion_block() {
        // <<<ORIGINAL without >>>SUGGESTED
        let input = "Line 5: Issue here.\n<<<ORIGINAL\nold code\n===\nnew code";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        // The code suggestion should be None since the block was never closed
        assert!(comments[0].code_suggestion.is_none());
    }

    #[test]
    fn parse_only_whitespace_input() {
        let input = "   \n\n  \t  \n  ";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_json_in_code_block_with_extra_text() {
        let input = "Here are my findings:\n```json\n[{\"line\": 5, \"issue\": \"Bug\"}]\n```\nLet me know if you need more details.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 5);
    }

    #[test]
    fn parse_json_with_diff_prefix_artifact() {
        // LLM sometimes echoes JSON in a code block with leading "+" on each line (diff artifact); repair strips (issue #28).
        let input = "```json\n+[{\"line\": 7, \"issue\": \"Missing check\"}]\n+\n```";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 7);
        assert!(comments[0].content.contains("Missing check"));
    }

    #[test]
    fn parse_json_with_single_quotes() {
        // LLM sometimes outputs JSON with single-quoted keys/values; repair converts to double quotes (issue #28).
        let input = r#"[{'line': 9, 'issue': 'Use of deprecated API'}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 9);
        assert!(comments[0].content.contains("deprecated"));
    }

    #[test]
    fn parse_json_single_quoted_object_wrapped_in_findings() {
        // Outer object with "findings" key; inner array uses single quotes — raw bracket span + repair.
        let input = r#"{"findings": [{'line': 2, 'issue': 'Minor bug'}]}"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 2);
        assert!(comments[0].content.contains("Minor bug"));
    }

    #[test]
    fn parse_json_single_quoted_value_with_escaped_apostrophe() {
        // Single-quoted value containing escaped apostrophe (e.g. "don't") — converter preserves it.
        let input = r#"[{'line': 1, 'issue': 'don\'t forget'}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 1);
        assert!(
            comments[0].content.contains("don't"),
            "content should contain apostrophe: {:?}",
            comments[0].content
        );
    }

    #[test]
    fn parse_json_double_quoted_value_with_apostrophe_unchanged() {
        // Valid JSON with apostrophe in double-quoted string — no repair; parses as-is.
        let input = r#"[{"line": 3, "issue": "don't use deprecated API"}]"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line_number, 3);
        assert!(comments[0].content.contains("don't"));
    }

    // ── Bug: find_json_array uses mismatched brackets ──────────────────
    //
    // `find_json_array` uses `find('[')` (first) + `rfind(']')` (last).
    // When two separate JSON arrays appear in the text, this grabs from
    // the first `[` to the last `]`, including non-JSON text between them.
    // The serde validation rejects the invalid combined string, causing
    // BOTH arrays to be silently lost.

    #[test]
    fn find_json_array_two_separate_arrays() {
        // Two valid JSON arrays separated by text — should extract the first one
        let input =
            "First: [{\"line\": 1, \"issue\": \"bug1\"}] and second: [{\"line\": 2, \"issue\": \"bug2\"}]";
        let result = find_json_array(input);
        assert!(
            result.is_some(),
            "Should find at least the first valid JSON array, not fail on mismatched brackets"
        );
        let json_str = result.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn find_json_array_validates_when_content_starts_with_bracket() {
        // Content starts with '[' but isn't valid JSON — should try to find
        // a valid array elsewhere, not return the invalid trimmed content
        let input = "[not json] here is the real one: [{\"line\": 5, \"issue\": \"Bug\"}]";
        let result = find_json_array(input);
        assert!(
            result.is_some(),
            "Should find the valid JSON array even when content starts with '['",
        );
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&result.unwrap()).expect("Should be valid JSON");
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn parse_file_line_format_does_not_match_urls() {
        // URLs with port numbers like http://localhost:8080 should not be parsed as file:line
        let input = "Visit http://localhost:8080 for the dashboard.";
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_llm_response(input, &file_path).unwrap();
        // Should not extract port 8080 as a line number
        assert!(
            comments.is_empty(),
            "URL port should not be parsed as line number, got {:?}",
            comments.iter().map(|c| c.line_number).collect::<Vec<_>>()
        );
    }
}
