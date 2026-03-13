use crate::adapters::llm::{LLMAdapter, LLMRequest};
use anyhow::Result;
use regex::Regex;

pub struct InteractiveCommand {
    pub command: CommandType,
    pub args: Vec<String>,
    #[allow(dead_code)] // Set by webhook handler when PR context is available
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandType {
    Review,
    Ignore,
    Explain,
    Generate,
    Help,
    Config,
}

impl InteractiveCommand {
    pub fn parse(comment: &str) -> Option<Self> {
        let command_regex = Regex::new(r"@diffscope\s+(\w+)(?:\s+(.*))?").ok()?;

        if let Some(captures) = command_regex.captures(comment) {
            let command_str = captures.get(1)?.as_str();
            let args_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");

            let command_type = match command_str.to_lowercase().as_str() {
                "review" => CommandType::Review,
                "ignore" => CommandType::Ignore,
                "explain" => CommandType::Explain,
                "generate" => CommandType::Generate,
                "help" => CommandType::Help,
                "config" => CommandType::Config,
                _ => return None,
            };

            let args = if args_str.is_empty() {
                Vec::new()
            } else {
                args_str.split_whitespace().map(String::from).collect()
            };

            Some(InteractiveCommand {
                command: command_type,
                args,
                context: None,
            })
        } else {
            None
        }
    }

    pub async fn execute(
        &self,
        adapter: &dyn LLMAdapter,
        diff_content: Option<&str>,
    ) -> Result<String> {
        match &self.command {
            CommandType::Review => self.execute_review(adapter, diff_content).await,
            CommandType::Ignore => self.execute_ignore(),
            CommandType::Explain => self.execute_explain(adapter, diff_content).await,
            CommandType::Generate => self.execute_generate(adapter).await,
            CommandType::Help => Ok(Self::get_help_text()),
            CommandType::Config => Ok(Self::get_config_info()),
        }
    }

    async fn execute_review(
        &self,
        adapter: &dyn LLMAdapter,
        diff_content: Option<&str>,
    ) -> Result<String> {
        if let Some(diff) = diff_content {
            let prompt = if self.args.is_empty() {
                format!("Review the following code changes:\n\n{}", diff)
            } else {
                let focus = self.args.join(" ");
                format!(
                    "Review the following code changes with focus on {}:\n\n{}",
                    focus, diff
                )
            };

            let request = LLMRequest {
                system_prompt: "You are a code reviewer. Provide concise, actionable feedback."
                    .to_string(),
                user_prompt: prompt,
                temperature: Some(0.3),
                max_tokens: Some(1000),
                response_schema: None,
            };

            let response = adapter.complete(request).await?;
            Ok(format!("## 🔍 Code Review\n\n{}", response.content))
        } else {
            Ok("No diff content available for review.".to_string())
        }
    }

    fn execute_ignore(&self) -> Result<String> {
        if self.args.is_empty() {
            Ok(
                "Please specify what to ignore (e.g., @diffscope ignore src/generated/)"
                    .to_string(),
            )
        } else {
            let patterns = self.args.join(", ");
            Ok(format!("✅ Will ignore: {}\n\nAdd these patterns to your .diffscope.yml for permanent configuration.", patterns))
        }
    }

    async fn execute_explain(
        &self,
        adapter: &dyn LLMAdapter,
        diff_content: Option<&str>,
    ) -> Result<String> {
        let context = if self.args.is_empty() {
            diff_content.unwrap_or("No specific context").to_string()
        } else {
            // Try to find specific line or section
            let target = self.args.join(" ");
            format!(
                "Explain the following in the context of the code changes: {}",
                target
            )
        };

        let request = LLMRequest {
            system_prompt:
                "You are a helpful code explainer. Provide clear, educational explanations."
                    .to_string(),
            user_prompt: format!("Explain this code or change:\n\n{}", context),
            temperature: Some(0.5),
            max_tokens: Some(800),
            response_schema: None,
        };

        let response = adapter.complete(request).await?;
        Ok(format!("## 💡 Explanation\n\n{}", response.content))
    }

    async fn execute_generate(&self, adapter: &dyn LLMAdapter) -> Result<String> {
        if self.args.is_empty() {
            return Ok(
                "Please specify what to generate (e.g., @diffscope generate tests)".to_string(),
            );
        }

        let target = self.args[0].as_str();
        let context = self.args[1..].join(" ");

        let (system_prompt, user_prompt) = match target {
            "tests" => (
                "You are a test generation expert. Generate comprehensive tests.",
                format!("Generate unit tests for the following context: {}", context),
            ),
            "docs" => (
                "You are a documentation expert. Generate clear, comprehensive documentation.",
                format!("Generate documentation for: {}", context),
            ),
            "types" => (
                "You are a TypeScript/type system expert. Generate proper type definitions.",
                format!("Generate type definitions for: {}", context),
            ),
            _ => (
                "You are a helpful code generator.",
                format!("Generate {} for: {}", target, context),
            ),
        };

        let request = LLMRequest {
            system_prompt: system_prompt.to_string(),
            user_prompt,
            temperature: Some(0.7),
            max_tokens: Some(1500),
            response_schema: None,
        };

        let response = adapter.complete(request).await?;
        Ok(format!(
            "## 🔨 Generated {}\n\n```\n{}\n```",
            target, response.content
        ))
    }

    pub fn help_text() -> String {
        Self::get_help_text()
    }

    fn get_help_text() -> String {
        r#"## 🤖 DiffScope Interactive Commands

Available commands:

### Review
- `@diffscope review` - Review the current changes
- `@diffscope review security` - Focus review on security aspects
- `@diffscope review performance` - Focus on performance

### Ignore
- `@diffscope ignore src/generated/` - Ignore files matching pattern
- `@diffscope ignore *.test.js` - Ignore test files

### Explain
- `@diffscope explain` - Explain the overall changes
- `@diffscope explain line 42` - Explain specific line
- `@diffscope explain function_name` - Explain specific function

### Generate
- `@diffscope generate tests` - Generate unit tests
- `@diffscope generate docs` - Generate documentation
- `@diffscope generate types` - Generate type definitions

### Other
- `@diffscope help` - Show this help message
- `@diffscope config` - Show current configuration"#
            .to_string()
    }

    fn get_config_info() -> String {
        r#"## ⚙️ Current Configuration

To configure DiffScope behavior, create a `.diffscope.yml` file:

```yaml
model: gpt-4o
temperature: 0.2
max_tokens: 4000

# Ignore patterns
exclude_patterns:
  - "**/*.generated.*"
  - "**/node_modules/**"
  
# Path-specific rules  
paths:
  "src/api/**":
    focus: ["security", "validation"]
  "tests/**":
    focus: ["coverage", "assertions"]
```

Interactive commands respect these configurations."#
            .to_string()
    }
}

/// Manages per-session ignore patterns from @diffscope ignore commands.
/// Will be wired into the review pipeline's triage filter.
#[allow(dead_code)]
pub struct InteractiveProcessor {
    /// Raw patterns for substring matching (no wildcards).
    literal_patterns: Vec<String>,
    /// Compiled regexes for glob patterns (contain `*`).
    glob_regexes: Vec<Regex>,
}

#[allow(dead_code)]
impl InteractiveProcessor {
    pub fn new() -> Self {
        Self {
            literal_patterns: Vec::new(),
            glob_regexes: Vec::new(),
        }
    }

    pub fn add_ignore_pattern(&mut self, pattern: &str) {
        if pattern.contains('*') {
            let escaped = regex::escape(pattern).replace(r"\*", ".*");
            let regex_pattern = format!("^{}$", escaped);
            if let Ok(re) = Regex::new(&regex_pattern) {
                self.glob_regexes.push(re);
            }
        } else {
            self.literal_patterns.push(pattern.to_string());
        }
    }

    pub fn should_ignore(&self, path: &str) -> bool {
        self.literal_patterns
            .iter()
            .any(|p| path.contains(p.as_str()))
            || self.glob_regexes.iter().any(|re| re.is_match(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === InteractiveCommand::parse tests ===

    #[test]
    fn test_parse_review_command() {
        let cmd = InteractiveCommand::parse("@diffscope review").unwrap();
        assert_eq!(cmd.command, CommandType::Review);
        assert!(cmd.args.is_empty());
        assert!(cmd.context.is_none());
    }

    #[test]
    fn test_parse_review_with_focus() {
        let cmd = InteractiveCommand::parse("@diffscope review security performance").unwrap();
        assert_eq!(cmd.command, CommandType::Review);
        assert_eq!(cmd.args, vec!["security", "performance"]);
    }

    #[test]
    fn test_parse_ignore_command() {
        let cmd = InteractiveCommand::parse("@diffscope ignore src/generated/").unwrap();
        assert_eq!(cmd.command, CommandType::Ignore);
        assert_eq!(cmd.args, vec!["src/generated/"]);
    }

    #[test]
    fn test_parse_explain_command() {
        let cmd = InteractiveCommand::parse("@diffscope explain").unwrap();
        assert_eq!(cmd.command, CommandType::Explain);
    }

    #[test]
    fn test_parse_generate_tests() {
        let cmd = InteractiveCommand::parse("@diffscope generate tests").unwrap();
        assert_eq!(cmd.command, CommandType::Generate);
        assert_eq!(cmd.args, vec!["tests"]);
    }

    #[test]
    fn test_parse_help() {
        let cmd = InteractiveCommand::parse("@diffscope help").unwrap();
        assert_eq!(cmd.command, CommandType::Help);
    }

    #[test]
    fn test_parse_config() {
        let cmd = InteractiveCommand::parse("@diffscope config").unwrap();
        assert_eq!(cmd.command, CommandType::Config);
    }

    #[test]
    fn test_parse_case_insensitive() {
        let cmd = InteractiveCommand::parse("@diffscope REVIEW").unwrap();
        assert_eq!(cmd.command, CommandType::Review);

        let cmd = InteractiveCommand::parse("@diffscope Help").unwrap();
        assert_eq!(cmd.command, CommandType::Help);
    }

    #[test]
    fn test_parse_unknown_command_returns_none() {
        assert!(InteractiveCommand::parse("@diffscope foobar").is_none());
    }

    #[test]
    fn test_parse_no_at_mention_returns_none() {
        assert!(InteractiveCommand::parse("just a regular comment").is_none());
    }

    #[test]
    fn test_parse_empty_string_returns_none() {
        assert!(InteractiveCommand::parse("").is_none());
    }

    #[test]
    fn test_parse_at_mention_only_returns_none() {
        assert!(InteractiveCommand::parse("@diffscope").is_none());
    }

    #[test]
    fn test_parse_embedded_in_text() {
        // Command embedded in longer comment
        let cmd =
            InteractiveCommand::parse("Hey team, can you @diffscope review this PR?").unwrap();
        assert_eq!(cmd.command, CommandType::Review);
    }

    #[test]
    fn test_parse_extra_whitespace() {
        let cmd = InteractiveCommand::parse("@diffscope   review   security").unwrap();
        assert_eq!(cmd.command, CommandType::Review);
        assert_eq!(cmd.args, vec!["security"]);
    }

    #[test]
    fn test_parse_different_at_mention_ignored() {
        assert!(InteractiveCommand::parse("@someone review").is_none());
    }

    // === execute_ignore tests ===

    #[test]
    fn test_ignore_no_args() {
        let cmd = InteractiveCommand {
            command: CommandType::Ignore,
            args: vec![],
            context: None,
        };
        let result = cmd.execute_ignore().unwrap();
        assert!(result.contains("specify what to ignore"));
    }

    #[test]
    fn test_ignore_with_patterns() {
        let cmd = InteractiveCommand {
            command: CommandType::Ignore,
            args: vec!["src/generated/".to_string(), "*.test.js".to_string()],
            context: None,
        };
        let result = cmd.execute_ignore().unwrap();
        assert!(result.contains("src/generated/"));
        assert!(result.contains("*.test.js"));
    }

    // === help_text / config tests ===

    #[test]
    fn test_help_text_contains_commands() {
        let help = InteractiveCommand::help_text();
        assert!(help.contains("@diffscope review"));
        assert!(help.contains("@diffscope ignore"));
        assert!(help.contains("@diffscope explain"));
        assert!(help.contains("@diffscope generate"));
        assert!(help.contains("@diffscope help"));
        assert!(help.contains("@diffscope config"));
    }

    #[test]
    fn test_config_info_contains_yaml() {
        let info = InteractiveCommand::get_config_info();
        assert!(info.contains(".diffscope.yml"));
        assert!(info.contains("exclude_patterns"));
    }

    // === InteractiveProcessor tests ===

    #[test]
    fn test_processor_new_empty() {
        let processor = InteractiveProcessor::new();
        assert!(!processor.should_ignore("any/path"));
    }

    #[test]
    fn test_processor_add_and_check_pattern() {
        let mut processor = InteractiveProcessor::new();
        processor.add_ignore_pattern("src/generated/");
        assert!(processor.should_ignore("src/generated/types.rs"));
        assert!(!processor.should_ignore("src/main.rs"));
    }

    #[test]
    fn test_processor_glob_pattern() {
        let mut processor = InteractiveProcessor::new();
        processor.add_ignore_pattern("*.test.js");
        assert!(processor.should_ignore("foo.test.js"));
        assert!(!processor.should_ignore("foo.js"));
    }

    #[test]
    fn test_processor_multiple_patterns() {
        let mut processor = InteractiveProcessor::new();
        processor.add_ignore_pattern("vendor/");
        processor.add_ignore_pattern("*.generated.*");
        assert!(processor.should_ignore("vendor/lib.js"));
        assert!(processor.should_ignore("types.generated.ts"));
        assert!(!processor.should_ignore("src/main.rs"));
    }

    // ── Bug: glob dots are not escaped before regex conversion ──────────
    //
    // `should_ignore` converts glob patterns to regex by replacing `*`
    // with `.*`, but does NOT escape the `.` characters in the pattern.
    // As a result, "*.test.js" becomes regex `".*..test..js"` where the
    // dots match ANY character, causing false positives.
    //
    // For example, "fooAtestBjs" matches because the unescaped dots in
    // the regex accept any character, not just literal periods.

    #[test]
    fn test_processor_glob_dot_is_literal() {
        let mut processor = InteractiveProcessor::new();
        processor.add_ignore_pattern("*.test.js");
        // Should match files with literal dots
        assert!(processor.should_ignore("foo.test.js"));
        // Should NOT match when dots are replaced by other characters
        assert!(
            !processor.should_ignore("fooAtestBjs"),
            "Glob dot should match literal '.' only, not arbitrary characters"
        );
    }

    #[test]
    fn test_processor_glob_anchored() {
        let mut processor = InteractiveProcessor::new();
        processor.add_ignore_pattern("*.test.js");
        // Should NOT match files with a suffix after .js
        assert!(
            !processor.should_ignore("foo.test.js.bak"),
            "Glob pattern should not match files with extra suffixes"
        );
    }
}
