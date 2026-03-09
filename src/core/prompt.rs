use crate::core::{LLMContextChunk, UnifiedDiff};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub user_prompt_template: String,
    pub max_tokens: usize,
    pub include_context: bool,
    pub max_context_chars: usize,
    pub max_diff_chars: usize,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: r#"You are an expert code reviewer with deep knowledge of software security, performance optimization, and best practices. Your role is to identify critical issues in code changes that could impact:
- Security (vulnerabilities, data exposure, injection risks)
- Correctness (bugs, logic errors, edge cases)
- Performance (inefficiencies, memory leaks, algorithmic complexity)
- Maintainability (code clarity, error handling, documentation)

Focus only on actionable issues. Do not comment on code style or formatting unless it impacts functionality."#.to_string(),
            user_prompt_template: r#"<task>
Review the code changes below and identify specific issues. Focus on problems that could cause bugs, security vulnerabilities, or performance issues.
</task>

<diff>
{diff}
</diff>

<context>
{context}
</context>

<instructions>
1. Analyze the changes systematically
2. For each issue found, provide:
   - Line number where the issue occurs
   - Clear description of the problem
   - Impact if not addressed
   - Optional rule id when a scoped review rule applies
   - Suggested fix (if applicable)
3. For every issue where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:

<<<ORIGINAL
<the problematic code, copied verbatim from the diff>
===
<the fixed code>
>>>SUGGESTED

Format each issue as:
Line [number] [rule:<id> optional]: [Issue type] - [Description]. [Impact]. [Suggestion if applicable].

Then, if a fix applies, add the code suggestion block on the next lines.

Examples:
Line 42 [rule:sec.sql.injection]: Security - User input passed directly to SQL query. Risk of SQL injection. Use parameterized queries.
<<<ORIGINAL
query = "SELECT * FROM users WHERE id = " + user_id
===
query = "SELECT * FROM users WHERE id = ?"
cursor.execute(query, (user_id,))
>>>SUGGESTED
Line 13: Bug - Missing null check before dereferencing pointer. May cause crash. Add null validation.
<<<ORIGINAL
value = obj.get_data()
result = value.process()
===
value = obj.get_data()
if value is not None:
    result = value.process()
>>>SUGGESTED
Line 28: Performance - O(n²) algorithm for large dataset. Will be slow with many items. Consider using a hash map.
</instructions>"#.to_string(),
            max_tokens: 2000,
            include_context: true,
            max_context_chars: 20000,
            max_diff_chars: 40000,
        }
    }
}

/// Build a system prompt focused exclusively on security issues.
pub fn build_security_prompt() -> String {
    r#"You are a security-focused code reviewer. Your ONLY job is to find security vulnerabilities in code changes. Do NOT comment on style, naming, performance, or general correctness.

Focus exclusively on:
- Injection attacks (SQL injection, XSS, command injection, LDAP injection)
- Authentication and authorization flaws (broken auth, missing access control, privilege escalation)
- Data exposure (secrets in code, PII leaks, sensitive data in logs, insecure storage)
- Cryptographic issues (weak algorithms, hardcoded keys, improper random number generation)
- OWASP Top 10 vulnerabilities (SSRF, insecure deserialization, security misconfiguration)
- Input validation failures (missing sanitization, path traversal, buffer overflows)
- Insecure communication (plaintext protocols, missing TLS verification)

Tag every finding with [security] at the start of the issue type.
If no security issues are found, respond with: No security issues found."#.to_string()
}

/// Build a system prompt focused exclusively on correctness issues.
pub fn build_correctness_prompt() -> String {
    r#"You are a correctness-focused code reviewer. Your ONLY job is to find bugs and logic errors in code changes. Do NOT comment on style, naming, or formatting.

Focus exclusively on:
- Logic errors (off-by-one, wrong operator, inverted conditions, unreachable code)
- Edge cases (empty collections, zero/negative values, boundary conditions, integer overflow)
- Null/None handling (null pointer dereference, unwrap on None/Err, missing Option checks)
- Concurrency issues (race conditions, deadlocks, data races, missing synchronization)
- Error handling (swallowed errors, incorrect error propagation, missing error cases)
- Resource management (unclosed handles, memory leaks, missing cleanup)
- Type safety (incorrect casts, lossy conversions, type confusion)
- API contract violations (precondition failures, invariant breaks)

Tag every finding with [correctness] at the start of the issue type.
If no correctness issues are found, respond with: No correctness issues found."#.to_string()
}

/// Build a system prompt focused exclusively on style and readability issues.
pub fn build_style_prompt() -> String {
    r#"You are a style-focused code reviewer. Your ONLY job is to find style, readability, and idiomatic code issues. Do NOT comment on bugs, security, or performance.

Focus exclusively on:
- Naming conventions (unclear variable/function names, inconsistent casing, abbreviations)
- Code patterns (non-idiomatic constructs, unnecessary complexity, missed language features)
- Readability (deeply nested code, overly long functions, unclear control flow)
- Consistency (mixed styles within the same file, inconsistent formatting)
- Dead code (unused imports, unreachable branches, commented-out code)
- Documentation (missing doc comments on public APIs, outdated comments, misleading names)

Tag every finding with [style] at the start of the issue type.
If no style issues are found, respond with: No style issues found."#.to_string()
}

/// Category label for a specialized review pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecializedPassKind {
    Security,
    Correctness,
    Style,
}

impl SpecializedPassKind {
    /// Human-readable tag added to comments produced by this pass.
    pub fn tag(self) -> &'static str {
        match self {
            SpecializedPassKind::Security => "security-pass",
            SpecializedPassKind::Correctness => "correctness-pass",
            SpecializedPassKind::Style => "style-pass",
        }
    }

    /// Build the specialized system prompt for this pass.
    pub fn system_prompt(self) -> String {
        match self {
            SpecializedPassKind::Security => build_security_prompt(),
            SpecializedPassKind::Correctness => build_correctness_prompt(),
            SpecializedPassKind::Style => build_style_prompt(),
        }
    }
}

pub struct PromptBuilder {
    config: PromptConfig,
}

impl PromptBuilder {
    pub fn new(config: PromptConfig) -> Self {
        Self { config }
    }

    pub fn build_prompt(
        &self,
        diff: &UnifiedDiff,
        context_chunks: &[LLMContextChunk],
    ) -> Result<(String, String)> {
        let diff_text = self.format_diff(diff)?;
        let context_text = if self.config.include_context {
            self.format_context(context_chunks)?
        } else {
            String::new()
        };

        let user_prompt = self
            .config
            .user_prompt_template
            .replace("{diff}", &diff_text)
            .replace("{context}", &context_text);

        Ok((self.config.system_prompt.clone(), user_prompt))
    }

    fn format_diff(&self, diff: &UnifiedDiff) -> Result<String> {
        let mut output = String::new();
        let mut truncated = false;
        output.push_str(&format!("File: {}\n", diff.file_path.display()));

        'hunks: for hunk in &diff.hunks {
            let header = format!("{}\n", hunk.context);
            if self.config.max_diff_chars > 0
                && output.len().saturating_add(header.len()) > self.config.max_diff_chars
            {
                truncated = true;
                break;
            }
            output.push_str(&header);

            for change in &hunk.changes {
                let prefix = match change.change_type {
                    crate::core::diff_parser::ChangeType::Added => "+",
                    crate::core::diff_parser::ChangeType::Removed => "-",
                    crate::core::diff_parser::ChangeType::Context => " ",
                };
                let line = format!("{}{}\n", prefix, change.content);
                if self.config.max_diff_chars > 0
                    && output.len().saturating_add(line.len()) > self.config.max_diff_chars
                {
                    truncated = true;
                    break 'hunks;
                }
                output.push_str(&line);
            }
        }

        if truncated {
            output.push_str("[Diff truncated]\n");
        }

        Ok(output)
    }

    fn format_context(&self, chunks: &[LLMContextChunk]) -> Result<String> {
        let mut output = String::new();

        for chunk in chunks {
            let block = format!(
                "\n[{:?} - {}{}]\n{}\n",
                chunk.context_type,
                chunk.file_path.display(),
                chunk
                    .line_range
                    .map(|(s, e)| format!(":{}-{}", s, e))
                    .unwrap_or_default(),
                chunk.content
            );
            if self.config.max_context_chars > 0
                && output.len().saturating_add(block.len()) > self.config.max_context_chars
            {
                output.push_str("\n[Context truncated]\n");
                break;
            }
            output.push_str(&block);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_prompt_focuses_on_security() {
        let prompt = build_security_prompt();
        assert!(prompt.contains("Injection"));
        assert!(prompt.contains("OWASP"));
        assert!(prompt.contains("[security]"));
        // Should NOT encourage style or correctness commentary
        assert!(prompt.contains("Do NOT comment on style"));
    }

    #[test]
    fn correctness_prompt_focuses_on_bugs() {
        let prompt = build_correctness_prompt();
        assert!(prompt.contains("Logic errors"));
        assert!(prompt.contains("Concurrency"));
        assert!(prompt.contains("[correctness]"));
        assert!(prompt.contains("Do NOT comment on style"));
    }

    #[test]
    fn style_prompt_focuses_on_readability() {
        let prompt = build_style_prompt();
        assert!(prompt.contains("Naming conventions"));
        assert!(prompt.contains("Readability"));
        assert!(prompt.contains("[style]"));
        assert!(prompt.contains("Do NOT comment on bugs"));
    }

    #[test]
    fn pass_kind_system_prompt_matches_builder() {
        assert_eq!(
            SpecializedPassKind::Security.system_prompt(),
            build_security_prompt()
        );
        assert_eq!(
            SpecializedPassKind::Correctness.system_prompt(),
            build_correctness_prompt()
        );
        assert_eq!(
            SpecializedPassKind::Style.system_prompt(),
            build_style_prompt()
        );
    }

    #[test]
    fn pass_kind_tags_are_unique() {
        let tags: Vec<&str> = vec![
            SpecializedPassKind::Security.tag(),
            SpecializedPassKind::Correctness.tag(),
            SpecializedPassKind::Style.tag(),
        ];
        let unique: std::collections::HashSet<&&str> = tags.iter().collect();
        assert_eq!(unique.len(), tags.len());
    }
}
