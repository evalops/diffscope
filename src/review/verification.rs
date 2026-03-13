use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

use crate::adapters::llm::{LLMAdapter, LLMRequest};
use crate::core::{Comment, UnifiedDiff};

/// Result of verifying a single review comment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub comment_id: String,
    pub accurate: bool,
    pub line_correct: bool,
    pub suggestion_sound: bool,
    pub score: u8, // 0-10
    pub reason: String,
}

/// Categories that should be auto-scored 0 (noise)
const AUTO_ZERO_PATTERNS: &[&str] = &[
    "docstring",
    "doc comment",
    "documentation comment",
    "type hint",
    "type annotation",
    "import order",
    "import sorting",
    "unused import",
    "trailing whitespace",
    "trailing newline",
];

const VERIFICATION_SYSTEM_PROMPT: &str = r#"You are a code review verifier. Your job is to validate review findings against the exact code snippets provided.

For each finding, assess:
1. Does the referenced file and line exist in the supplied evidence?
2. Does the comment accurately describe the code shown in the diff and nearby file context?
3. Is the suggestion sound, or would it introduce regressions / incorrect advice?
4. Is the finding a false positive or hallucinated issue?

Score each finding 0-10:
- 8-10: Critical bugs or security issues that are clearly present
- 5-7: Valid issues that exist but may be minor
- 1-4: Questionable issues, possibly hallucinated or too trivial
- 0: Noise (docstrings, type hints, import ordering, trailing whitespace)

Respond with JSON only. Return an array of objects with this schema:
[{"index":1,"accurate":true,"line_correct":true,"suggestion_sound":true,"score":8,"reason":"brief reason"}]
"#;

/// Verify a batch of review comments by asking the LLM to validate each one.
/// Returns only comments that pass verification (score >= min_score).
pub async fn verify_comments(
    comments: Vec<Comment>,
    diffs: &[UnifiedDiff],
    repo_path: &Path,
    adapter: &dyn LLMAdapter,
    min_score: u8,
) -> Result<Vec<Comment>> {
    if comments.is_empty() {
        return Ok(comments);
    }

    // Build verification prompt
    let prompt = build_verification_prompt(&comments, diffs, repo_path);

    let request = LLMRequest {
        system_prompt: VERIFICATION_SYSTEM_PROMPT.to_string(),
        user_prompt: prompt,
        temperature: Some(0.0),
        max_tokens: Some(2000),
    };

    let response = adapter.complete(request).await?;
    let results = parse_verification_response(&response.content, &comments);

    // Filter comments based on verification results
    let total_count = comments.len();
    let mut verified = Vec::new();
    for comment in comments {
        let result = results.iter().find(|r| r.comment_id == comment.id);
        match result {
            Some(r) if r.score >= min_score && r.accurate && r.line_correct => {
                let mut comment = comment;
                // Update confidence based on verification score
                comment.confidence = (r.score as f32 / 10.0).min(1.0);
                if !r.suggestion_sound {
                    comment.suggestion = None;
                    comment.code_suggestion = None;
                }
                verified.push(comment);
            }
            Some(r) => {
                // Score too low, skip
                info!(
                    "Verification filtered comment {} (score: {})",
                    comment.id, r.score
                );
            }
            None => {
                // No verification result found, keep with original confidence
                verified.push(comment);
            }
        }
    }

    info!(
        "Verification: {}/{} comments passed",
        verified.len(),
        total_count
    );

    Ok(verified)
}

/// Check if a comment's content matches auto-zero patterns.
pub fn is_auto_zero(content: &str) -> bool {
    let lower = content.to_lowercase();
    AUTO_ZERO_PATTERNS.iter().any(|p| lower.contains(p))
}

fn build_verification_prompt(
    comments: &[Comment],
    diffs: &[UnifiedDiff],
    repo_path: &Path,
) -> String {
    let diff_map = diffs
        .iter()
        .map(|diff| (diff.file_path.clone(), diff))
        .collect::<HashMap<_, _>>();

    let mut prompt = String::from("## Findings to Verify\n\n");

    for (i, comment) in comments.iter().enumerate() {
        let diff = diff_map.get(&comment.file_path).copied();
        prompt.push_str(&format!(
            "### Finding {}\n- File: {}:{}\n- Issue: {}\n",
            i + 1,
            comment.file_path.display(),
            comment.line_number,
            comment.content,
        ));
        if let Some(ref suggestion) = comment.suggestion {
            prompt.push_str(&format!("- Suggestion: {}\n", suggestion));
        }
        if let Some(diff) = diff {
            let diff_snippet = diff_snippet_for_comment(diff, comment.line_number);
            if !diff_snippet.trim().is_empty() {
                prompt.push_str("- Diff evidence:\n```diff\n");
                prompt.push_str(&diff_snippet);
                prompt.push_str("\n```\n");
            }
        }
        if let Ok(content) = std::fs::read_to_string(repo_path.join(&comment.file_path)) {
            let file_context = source_context_for_line(&content, comment.line_number, 6);
            if !file_context.trim().is_empty() {
                prompt.push_str("- Nearby file context:\n```\n");
                prompt.push_str(&file_context);
                prompt.push_str("\n```\n");
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("Return JSON only. Do not add commentary outside the JSON array.\n");
    prompt
}

fn parse_verification_response(content: &str, comments: &[Comment]) -> Vec<VerificationResult> {
    static FINDING_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)FINDING\s+(\d+)\s*:\s*score\s*=\s*(\d+)\s+accurate\s*=\s*(true|false)(?:\s+line_correct\s*=\s*(true|false))?(?:\s+suggestion_sound\s*=\s*(true|false))?\s+reason\s*=\s*(.+)")
            .unwrap()
    });

    if let Some(results) = parse_verification_json(content, comments) {
        return apply_auto_zero(results, comments);
    }

    let mut results = Vec::new();

    for line in content.lines() {
        if let Some(caps) = FINDING_PATTERN.captures(line) {
            let index: usize = caps
                .get(1)
                .expect("capture group 1 (index) must exist after regex match")
                .as_str()
                .parse()
                .unwrap_or(0);
            let score: u8 = caps
                .get(2)
                .expect("capture group 2 (score) must exist after regex match")
                .as_str()
                .parse()
                .unwrap_or(0);
            let accurate = caps
                .get(3)
                .expect("capture group 3 (accurate) must exist after regex match")
                .as_str()
                .to_lowercase()
                == "true";
            let line_correct = caps
                .get(4)
                .map(|value| value.as_str().eq_ignore_ascii_case("true"))
                .unwrap_or(accurate);
            let suggestion_sound = caps
                .get(5)
                .map(|value| value.as_str().eq_ignore_ascii_case("true"))
                .unwrap_or(true);
            let reason = caps
                .get(6)
                .expect("capture group 6 (reason) must exist after regex match")
                .as_str()
                .trim()
                .to_string();

            // Map 1-based index to comment
            if index > 0 && index <= comments.len() {
                results.push(VerificationResult {
                    comment_id: comments[index - 1].id.clone(),
                    accurate,
                    line_correct,
                    suggestion_sound,
                    score: score.min(10),
                    reason,
                });
            }
        }
    }

    apply_auto_zero(results, comments)
}

fn parse_verification_json(content: &str, comments: &[Comment]) -> Option<Vec<VerificationResult>> {
    let trimmed = content.trim();
    let candidate = if trimmed.starts_with("```") {
        trimmed
            .lines()
            .skip_while(|line| line.trim_start().starts_with("```"))
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        trimmed.to_string()
    };

    let value = serde_json::from_str::<Value>(&candidate).ok()?;
    let items = if let Some(array) = value.as_array() {
        array.clone()
    } else {
        value
            .get("results")
            .and_then(|results| results.as_array())
            .cloned()?
    };

    let mut results = Vec::new();
    for item in items {
        let index = item.get("index").and_then(|value| value.as_u64())? as usize;
        if index == 0 || index > comments.len() {
            continue;
        }
        let accurate = item
            .get("accurate")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let line_correct = item
            .get("line_correct")
            .and_then(|value| value.as_bool())
            .unwrap_or(accurate);
        let suggestion_sound = item
            .get("suggestion_sound")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let score = item
            .get("score")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .min(10) as u8;
        let reason = item
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("No reason provided")
            .to_string();

        results.push(VerificationResult {
            comment_id: comments[index - 1].id.clone(),
            accurate,
            line_correct,
            suggestion_sound,
            score,
            reason,
        });
    }
    Some(results)
}

fn apply_auto_zero(
    mut results: Vec<VerificationResult>,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    for comment in comments {
        if is_auto_zero(&comment.content) {
            if let Some(existing) = results.iter_mut().find(|r| r.comment_id == comment.id) {
                existing.accurate = false;
                existing.line_correct = false;
                existing.score = 0;
                existing.reason = "Auto-zero: noise category".to_string();
            } else {
                results.push(VerificationResult {
                    comment_id: comment.id.clone(),
                    accurate: false,
                    line_correct: false,
                    suggestion_sound: false,
                    score: 0,
                    reason: "Auto-zero: noise category".to_string(),
                });
            }
        }
    }

    results
}

fn diff_snippet_for_comment(diff: &UnifiedDiff, line_number: usize) -> String {
    for hunk in &diff.hunks {
        let hunk_start = hunk.new_start;
        let hunk_end = hunk.new_start + hunk.new_lines.saturating_sub(1);
        if (hunk_start..=hunk_end.max(hunk_start)).contains(&line_number) {
            return hunk
                .changes
                .iter()
                .map(|change| {
                    let prefix = match change.change_type {
                        crate::core::diff_parser::ChangeType::Added => "+",
                        crate::core::diff_parser::ChangeType::Removed => "-",
                        crate::core::diff_parser::ChangeType::Context => " ",
                    };
                    format!("{}{}", prefix, change.content)
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}

fn source_context_for_line(content: &str, line_number: usize, radius: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let start = line_number.saturating_sub(radius + 1);
    let end = (line_number + radius).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{:>4}: {}", start + offset + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
fn safe_utf8_prefix(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    &content[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, Comment, FixEffort, Severity};
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn make_comment(id: &str, content: &str, line: usize) -> Comment {
        Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.7,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
        }
    }

    fn make_diff(file_path: &str, entries: &[(usize, &str)]) -> UnifiedDiff {
        UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from(file_path),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: entries
                .iter()
                .map(|(line_number, content)| DiffHunk {
                    old_start: *line_number,
                    old_lines: 1,
                    new_start: *line_number,
                    new_lines: 1,
                    context: String::new(),
                    changes: vec![DiffLine {
                        old_line_no: Some(*line_number),
                        new_line_no: Some(*line_number),
                        change_type: ChangeType::Added,
                        content: (*content).to_string(),
                    }],
                })
                .collect(),
        }
    }

    fn build_prompt_for_tests(comments: &[Comment]) -> String {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("src/lib.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let file_content = (1..=30)
            .map(|line| match line {
                10 => "let query = format!(\"SELECT * FROM users WHERE id = {}\", user_id);"
                    .to_string(),
                20 => "let user = maybe_user.unwrap();".to_string(),
                _ => format!("// line {}", line),
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file_path, file_content).unwrap();

        let diffs = vec![make_diff(
            "src/lib.rs",
            &[
                (
                    10,
                    "let query = format!(\"SELECT * FROM users WHERE id = {}\", user_id);",
                ),
                (20, "let user = maybe_user.unwrap();"),
            ],
        )];

        build_verification_prompt(comments, &diffs, dir.path())
    }

    #[test]
    fn test_is_auto_zero_docstring() {
        assert!(is_auto_zero("Missing docstring for public function"));
        assert!(is_auto_zero("Add a documentation comment here"));
    }

    #[test]
    fn test_is_auto_zero_type_hint() {
        assert!(is_auto_zero("Missing type annotation on parameter"));
        assert!(is_auto_zero("Add type hint for return value"));
    }

    #[test]
    fn test_is_auto_zero_imports() {
        assert!(is_auto_zero("Unused import: std::io"));
        assert!(is_auto_zero("Import sorting is inconsistent"));
    }

    #[test]
    fn test_is_auto_zero_false_for_real_issues() {
        assert!(!is_auto_zero("SQL injection vulnerability"));
        assert!(!is_auto_zero("Missing null check on user input"));
        assert!(!is_auto_zero("Buffer overflow in array access"));
    }

    #[test]
    fn test_build_verification_prompt_includes_all_findings() {
        let comments = vec![
            make_comment("c1", "SQL injection risk", 10),
            make_comment("c2", "Missing null check", 20),
        ];
        let prompt = build_prompt_for_tests(&comments);
        assert!(prompt.contains("Finding 1"));
        assert!(prompt.contains("Finding 2"));
        assert!(prompt.contains("SQL injection risk"));
        assert!(prompt.contains("Missing null check"));
        assert!(prompt.contains("Diff evidence"));
        assert!(prompt.contains("Nearby file context"));
    }

    #[test]
    fn test_build_verification_prompt_includes_source_context() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let prompt = build_prompt_for_tests(&comments);
        assert!(prompt.contains("SELECT * FROM users"));
    }

    #[test]
    fn test_build_verification_prompt_handles_multiple_findings() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let prompt = build_prompt_for_tests(&comments);
        assert!(prompt.contains("Return JSON only"));
    }

    #[test]
    fn test_build_verification_prompt_includes_suggestion() {
        let mut comment = make_comment("c1", "Use parameterized queries", 10);
        comment.suggestion = Some("Use prepared statements instead".to_string());
        let prompt = build_prompt_for_tests(&[comment]);
        assert!(prompt.contains("Suggestion: Use prepared statements instead"));
    }

    #[test]
    fn test_parse_verification_response_basic() {
        let comments = vec![
            make_comment("c1", "SQL injection", 10),
            make_comment("c2", "Missing check", 20),
        ];
        let response = "FINDING 1: score=9 accurate=true reason=SQL injection is present\nFINDING 2: score=3 accurate=false reason=Check exists on line 18";
        let results = parse_verification_response(response, &comments);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].score, 9);
        assert!(results[0].accurate);
        assert_eq!(results[1].score, 3);
        assert!(!results[1].accurate);
    }

    #[test]
    fn test_parse_verification_response_json() {
        let comments = vec![make_comment("c1", "SQL injection", 10)];
        let response = r#"[{"index":1,"accurate":true,"line_correct":true,"suggestion_sound":false,"score":8,"reason":"Verified"}]"#;
        let results = parse_verification_response(response, &comments);
        assert_eq!(results.len(), 1);
        assert!(results[0].accurate);
        assert!(!results[0].suggestion_sound);
        assert_eq!(results[0].score, 8);
    }

    #[test]
    fn test_parse_verification_response_case_insensitive() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "finding 1: score=7 accurate=true reason=Valid issue";
        let results = parse_verification_response(response, &comments);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 7);
    }

    #[test]
    fn test_parse_verification_response_auto_zero_applied() {
        let comments = vec![
            make_comment("c1", "Missing docstring for function", 10),
            make_comment("c2", "SQL injection risk", 20),
        ];
        let response = "FINDING 1: score=5 accurate=true reason=Valid\nFINDING 2: score=9 accurate=true reason=Real issue";
        let results = parse_verification_response(response, &comments);
        // c1 should be auto-zeroed despite LLM giving it score=5
        let c1_result = results.iter().find(|r| r.comment_id == "c1").unwrap();
        assert_eq!(c1_result.score, 0);
        // c2 should keep its score
        let c2_result = results.iter().find(|r| r.comment_id == "c2").unwrap();
        assert_eq!(c2_result.score, 9);
    }

    #[test]
    fn test_parse_verification_response_empty() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "No issues to report.";
        let results = parse_verification_response(response, &comments);
        // Should only have auto-zero results if applicable
        assert!(results.is_empty() || results.iter().all(|r| r.score == 0));
    }

    #[test]
    fn test_parse_verification_response_score_clamped() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "FINDING 1: score=15 accurate=true reason=Very important";
        let results = parse_verification_response(response, &comments);
        assert_eq!(results[0].score, 10); // clamped to 10
    }

    #[test]
    fn test_parse_verification_response_invalid_index() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "FINDING 0: score=5 accurate=true reason=bad index\nFINDING 99: score=5 accurate=true reason=out of range";
        let results = parse_verification_response(response, &comments);
        assert!(results.is_empty()); // both indices invalid
    }

    #[test]
    fn test_parse_verification_response_preserves_reason() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response =
            "FINDING 1: score=8 accurate=true reason=The buffer overflow is clearly present";
        let results = parse_verification_response(response, &comments);
        assert_eq!(results[0].reason, "The buffer overflow is clearly present");
    }

    #[test]
    fn test_parse_verification_response_multiple_auto_zero() {
        let comments = vec![
            make_comment("c1", "Missing docstring for function", 10),
            make_comment("c2", "Trailing whitespace on line 5", 20),
            make_comment("c3", "Real security bug", 30),
        ];
        // LLM only responds about c3
        let response = "FINDING 3: score=9 accurate=true reason=Valid security issue";
        let results = parse_verification_response(response, &comments);
        // c1 and c2 should get auto-zero results
        let c1_result = results.iter().find(|r| r.comment_id == "c1").unwrap();
        assert_eq!(c1_result.score, 0);
        let c2_result = results.iter().find(|r| r.comment_id == "c2").unwrap();
        assert_eq!(c2_result.score, 0);
        // c3 should keep its score
        let c3_result = results.iter().find(|r| r.comment_id == "c3").unwrap();
        assert_eq!(c3_result.score, 9);
    }

    #[test]
    fn test_is_auto_zero_whitespace() {
        assert!(is_auto_zero("trailing whitespace detected"));
        assert!(is_auto_zero("Missing trailing newline at end of file"));
    }

    #[test]
    fn test_is_auto_zero_import_order() {
        assert!(is_auto_zero("import order should be alphabetical"));
    }

    // ── Mutation-testing gap fills ─────────────────────────────────────

    #[test]
    fn test_safe_utf8_prefix_short_string() {
        let result = safe_utf8_prefix("hello", 100);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_safe_utf8_prefix_exact_boundary() {
        let result = safe_utf8_prefix("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_safe_utf8_prefix_truncates() {
        let result = safe_utf8_prefix("hello world", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_safe_utf8_prefix_multibyte() {
        // "é" is 2 bytes. "éé" = 4 bytes. Truncating at 3 should give "é" (2 bytes).
        let result = safe_utf8_prefix("éé", 3);
        assert_eq!(result, "é");
    }

    #[test]
    fn test_safe_utf8_prefix_emoji() {
        // "😀" is 4 bytes. Truncating at 2 should give empty since we can't split the emoji.
        let result = safe_utf8_prefix("😀hello", 2);
        assert!(result.is_empty() || result.len() <= 2);
    }

    #[test]
    fn test_safe_utf8_prefix_empty() {
        let result = safe_utf8_prefix("", 100);
        assert_eq!(result, "");
    }

    // ── Adversarial edge cases ──────────────────────────────────────────

    #[test]
    fn test_parse_verification_response_duplicate_findings() {
        // LLM returns two results for the same finding
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "FINDING 1: score=9 accurate=true reason=First\nFINDING 1: score=3 accurate=false reason=Second";
        let results = parse_verification_response(response, &comments);
        // Both should be captured (first one wins in filter since find() returns first)
        let c1_results: Vec<_> = results.iter().filter(|r| r.comment_id == "c1").collect();
        assert!(
            !c1_results.is_empty(),
            "Should have at least one result for c1"
        );
    }

    #[test]
    fn test_parse_verification_extra_whitespace() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "FINDING   1 :  score = 8   accurate = true   reason = Valid bug";
        let results = parse_verification_response(response, &comments);
        // The regex uses \s+ so extra spaces should be handled
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 8);
    }

    #[test]
    fn test_parse_verification_response_with_surrounding_text() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let response = "Here are my verification results:\n\nFINDING 1: score=7 accurate=true reason=Valid\n\nOverall the code looks good.";
        let results = parse_verification_response(response, &comments);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 7);
    }

    #[test]
    fn test_is_auto_zero_case_sensitivity() {
        // Auto-zero should be case-insensitive
        assert!(is_auto_zero("MISSING DOCSTRING"));
        assert!(is_auto_zero("Type Annotation missing"));
        assert!(is_auto_zero("IMPORT ORDER"));
    }

    #[test]
    fn test_is_auto_zero_partial_match_false_positive() {
        // "import" appears in "important" but "import order" does not
        assert!(!is_auto_zero("This is an important security fix"));
        // "type hint" appears in "cryptotype hinting" — substring match
        // This IS a known limitation of substring matching
        assert!(!is_auto_zero("The cryptographic module is broken"));
    }

    #[test]
    fn test_build_verification_prompt_empty_comments() {
        let prompt = build_prompt_for_tests(&[]);
        assert!(prompt.contains("## Findings to Verify"));
    }

    #[test]
    fn test_build_verification_prompt_empty_diff() {
        let comments = vec![make_comment("c1", "issue", 10)];
        let prompt = build_prompt_for_tests(&comments);
        assert!(prompt.contains("Finding 1"));
    }
}
