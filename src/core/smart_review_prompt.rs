#![allow(dead_code)]

use crate::core::{LLMContextChunk, UnifiedDiff};
use anyhow::Result;

pub struct SmartReviewPromptBuilder;

impl SmartReviewPromptBuilder {
    pub fn build_enhanced_review_prompt(
        diff: &UnifiedDiff,
        context_chunks: &[LLMContextChunk],
        max_context_chars: usize,
        max_diff_chars: usize,
        system_prompt_suffix: Option<&str>,
    ) -> Result<(String, String)> {
        let mut system_prompt = Self::build_smart_review_system_prompt();
        if let Some(suffix) = system_prompt_suffix {
            let trimmed = suffix.trim();
            if !trimmed.is_empty() {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(trimmed);
            }
        }
        let user_prompt = Self::build_smart_review_user_prompt(
            diff,
            context_chunks,
            max_context_chars,
            max_diff_chars,
        )?;

        Ok((system_prompt, user_prompt))
    }

    fn build_smart_review_system_prompt() -> String {
        r#"You are an advanced AI code reviewer with expertise in security, performance, maintainability, and best practices. 

Core rules:
- Only report issues with concrete evidence in the diff or provided context
- Do not speculate about code you cannot see
- If a sanitizer, guard, or safe pattern is clearly present, do not flag the issue
- Prefer fewer high-confidence findings over many low-confidence ones
- Do not write generic advice that starts with Ensure, Verify, Validate, Consider, Review, or Confirm
- State the concrete problem and the smallest safe fix instead of open-ended review tasks

Your task is to provide intelligent code reviews that are:
1. **Actionable**: Each comment should provide specific, implementable suggestions
2. **Contextual**: Consider the broader codebase context and patterns
3. **Prioritized**: Focus on issues that matter most for code quality and security
4. **Educational**: Explain the reasoning behind suggestions to help developers learn

## Analysis Framework

### Severity Levels:
- **CRITICAL**: Security vulnerabilities, potential crashes, data loss risks
- **HIGH**: Bugs, performance issues, major maintainability problems  
- **MEDIUM**: Code quality issues, minor performance concerns, best practice violations
- **LOW**: Style preferences, documentation improvements, optional optimizations

### Categories:
- **Security**: Authentication, authorization, input validation, injection vulnerabilities
- **Performance**: Algorithmic efficiency, memory usage, database queries, caching
- **Bug**: Logic errors, edge cases, null pointer issues, race conditions
- **Maintainability**: Code complexity, readability, modularity, error handling
- **Testing**: Test coverage, test quality, testability
- **Style**: Naming, formatting, code organization
- **Documentation**: Comments, API docs, README updates

## Response Format

For each issue found, respond with exactly this format:

```
ISSUE: [Brief title]
LINE: [line number]
RULE: [optional rule id or empty]
SEVERITY: [CRITICAL|HIGH|MEDIUM|LOW]
CATEGORY: [Security|Performance|Bug|Maintainability|Testing|Style|Documentation]
CONFIDENCE: [0-100]%
EFFORT: [Low|Medium|High]

DESCRIPTION:
[Detailed explanation of the issue and why it matters]

SUGGESTION:
[Specific, actionable fix with code examples if applicable]

TAGS: [comma-separated relevant tags]
```

## Guidelines:
- Only report real issues, not false positives
- Provide code examples in suggestions when helpful
- Consider the file type and language-specific best practices
- Be concise but thorough in explanations
- Focus on issues that improve security, reliability, or maintainability"#.to_string()
    }

    fn build_smart_review_user_prompt(
        diff: &UnifiedDiff,
        context_chunks: &[LLMContextChunk],
        max_context_chars: usize,
        max_diff_chars: usize,
    ) -> Result<String> {
        let mut prompt = String::new();
        let mut context_chars = 0usize;
        let mut diff_chars = 0usize;
        let mut diff_truncated = false;

        prompt.push_str(&format!(
            "Please review the following code changes in file: {}\n\n",
            diff.file_path.display()
        ));

        // Add context information
        if !context_chunks.is_empty() {
            prompt.push_str("## Context Information\n\n");
            for chunk in context_chunks {
                let (start_line, end_line) = chunk.line_range.unwrap_or((1, 1));
                let description =
                    format!("{} - {:?}", chunk.file_path.display(), chunk.context_type);
                let block = format!(
                    "**{}** (lines {}-{}):\n```\n{}\n```\n\n",
                    description,
                    start_line,
                    end_line,
                    chunk
                        .content
                        .lines()
                        .take(20)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                if max_context_chars > 0
                    && context_chars.saturating_add(block.len()) > max_context_chars
                {
                    prompt.push_str("[Context truncated]\n\n");
                    break;
                }
                prompt.push_str(&block);
                context_chars = context_chars.saturating_add(block.len());
            }
        }

        prompt.push_str("## Code Changes\n\n");

        // Format the diff with line numbers and change indicators
        for hunk in &diff.hunks {
            let new_end = if hunk.new_lines == 0 {
                hunk.new_start
            } else {
                hunk.new_start + hunk.new_lines - 1
            };
            let old_end = if hunk.old_lines == 0 {
                hunk.old_start
            } else {
                hunk.old_start + hunk.old_lines - 1
            };
            let hunk_header = format!(
                "### Hunk: Lines {}-{} (was {}-{})\n\n",
                hunk.new_start, new_end, hunk.old_start, old_end
            );
            if max_diff_chars > 0 && diff_chars.saturating_add(hunk_header.len()) > max_diff_chars {
                diff_truncated = true;
                break;
            }
            prompt.push_str(&hunk_header);
            diff_chars = diff_chars.saturating_add(hunk_header.len());

            if max_diff_chars > 0 && diff_chars.saturating_add("```diff\n".len()) > max_diff_chars {
                diff_truncated = true;
                break;
            }
            prompt.push_str("```diff\n");
            diff_chars = diff_chars.saturating_add("```diff\n".len());
            let mut line_num = hunk.new_start;

            for line in &hunk.changes {
                let prefix = match line.change_type {
                    crate::core::diff_parser::ChangeType::Added => "+",
                    crate::core::diff_parser::ChangeType::Removed => "-",
                    crate::core::diff_parser::ChangeType::Context => " ",
                };

                let rendered = format!("{}{:4} {}\n", prefix, line_num, line.content);
                if max_diff_chars > 0 && diff_chars.saturating_add(rendered.len()) > max_diff_chars
                {
                    diff_truncated = true;
                    break;
                }
                prompt.push_str(&rendered);
                diff_chars = diff_chars.saturating_add(rendered.len());

                if !matches!(
                    line.change_type,
                    crate::core::diff_parser::ChangeType::Removed
                ) {
                    line_num += 1;
                }
            }

            prompt.push_str("```\n\n");
            diff_chars = diff_chars.saturating_add("```\n\n".len());

            if diff_truncated {
                break;
            }
        }

        if diff_truncated {
            prompt.push_str("[Diff truncated]\n\n");
        }

        prompt.push_str("## Review Instructions\n\n");
        prompt.push_str("Please analyze the code changes for:\n");
        prompt.push_str(
            "1. Security vulnerabilities (SQL injection, XSS, authentication bypass, etc.)\n",
        );
        prompt.push_str(
            "2. Performance issues (N+1 queries, inefficient algorithms, memory leaks)\n",
        );
        prompt.push_str("3. Bugs and edge cases (null pointers, race conditions, logic errors)\n");
        prompt.push_str("4. Maintainability concerns (complexity, readability, error handling)\n");
        prompt.push_str("5. Testing gaps (missing tests, poor test quality)\n");
        prompt.push_str("6. Best practice violations (naming, patterns, architecture)\n\n");

        prompt.push_str("Focus on the most impactful issues. Provide specific, actionable suggestions with code examples where helpful.\n");

        Ok(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine};
    use std::path::PathBuf;

    // ── Bug: hunk header shows one-past-end line number ──────────────
    //
    // A hunk with new_start=10, new_lines=5 covers lines 10-14 (inclusive).
    // The old code displayed "Lines 10-15", which is off by one.

    #[test]
    fn test_hunk_header_line_range_is_inclusive() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 5,
                old_lines: 3,
                new_start: 10,
                new_lines: 5,
                context: String::new(),
                changes: vec![DiffLine {
                    content: "line".to_string(),
                    change_type: ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(10),
                }],
            }],
        };
        let (_system, user) =
            SmartReviewPromptBuilder::build_enhanced_review_prompt(&diff, &[], 1000, 10000, None)
                .unwrap();
        // Should say "Lines 10-14" (inclusive), not "Lines 10-15"
        assert!(
            user.contains("Lines 10-14"),
            "Hunk header should use inclusive end line; prompt contains: {}",
            user.lines()
                .find(|l| l.contains("Hunk:"))
                .unwrap_or("(no hunk header found)")
        );
        assert!(
            user.contains("was 5-7"),
            "Old range should also be inclusive: 5 + 3 - 1 = 7"
        );
    }

    #[test]
    fn test_hunk_header_zero_lines_no_overflow() {
        // Deletion-only hunk: new_lines=0 must not produce usize::MAX via
        // saturating_sub(1) on 0.
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 10,
                old_lines: 3,
                new_start: 10,
                new_lines: 0, // deletion-only
                context: String::new(),
                changes: vec![DiffLine {
                    content: "removed".to_string(),
                    change_type: ChangeType::Removed,
                    old_line_no: Some(10),
                    new_line_no: None,
                }],
            }],
        };
        let (_system, user) =
            SmartReviewPromptBuilder::build_enhanced_review_prompt(&diff, &[], 1000, 10000, None)
                .unwrap();
        // Should say "Lines 10-10", NOT "Lines 10-18446744073709551615"
        assert!(
            user.contains("Lines 10-10"),
            "Zero new_lines should produce start-start range; got: {}",
            user.lines()
                .find(|l| l.contains("Hunk:"))
                .unwrap_or("(no hunk header found)")
        );
    }

    #[test]
    fn smart_review_system_prompt_discourages_vague_advice() {
        let (system, _) = SmartReviewPromptBuilder::build_enhanced_review_prompt(
            &UnifiedDiff {
                file_path: PathBuf::from("test.rs"),
                old_content: None,
                new_content: None,
                is_new: false,
                is_deleted: false,
                is_binary: false,
                hunks: vec![],
            },
            &[],
            1000,
            10000,
            None,
        )
        .unwrap();

        assert!(system.contains("Do not write generic advice that starts with Ensure"));
        assert!(system.contains("smallest safe fix"));
    }
}
