use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::adapters;
use crate::config;
use crate::core;

use super::types::DiscussionThread;

const MAX_DISCUSSION_RULE_CANDIDATES: usize = 2;
const MAX_DISCUSSION_CONTEXT_CANDIDATES: usize = 2;
const DISCUSSION_DERIVED_TAG: &str = "discussion-derived";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(super) struct DiscussionCandidateRule {
    pub(super) id: String,
    pub(super) description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub(super) struct DiscussionCandidateSuggestions {
    pub(super) summary: String,
    pub(super) rules: Vec<DiscussionCandidateRule>,
    pub(super) custom_context: Vec<config::CustomContextConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawDiscussionCandidateSuggestions {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    rules: Vec<DiscussionCandidateRule>,
    #[serde(default)]
    custom_context: Vec<config::CustomContextConfig>,
}

pub(super) fn has_user_discussion_turns(thread: &DiscussionThread) -> bool {
    thread
        .turns
        .iter()
        .any(|turn| turn.role.eq_ignore_ascii_case("user") && !turn.message.trim().is_empty())
}

pub(super) async fn suggest_discussion_candidates(
    adapter: &dyn adapters::llm::LLMAdapter,
    comment: &core::Comment,
    thread: &DiscussionThread,
) -> Result<DiscussionCandidateSuggestions> {
    let request = adapters::llm::LLMRequest {
        system_prompt: "You turn follow-up review discussions into reusable DiffScope rule and custom-context candidates. Only suggest durable guidance that should be reused in future reviews. Prefer empty arrays over weak or one-off guidance. Do not mention line numbers, commit SHAs, or PR-specific details in candidate descriptions.".to_string(),
        user_prompt: build_candidate_prompt(comment, thread),
        temperature: Some(0.1),
        max_tokens: Some(1800),
        response_schema: Some(discussion_candidate_response_schema()),
    };

    let response = adapter.complete(request).await?;
    let parsed = parse_candidate_response(&response.content)?;
    Ok(normalize_candidate_suggestions(comment, parsed))
}

pub(super) fn print_discussion_candidates(
    suggestions: &DiscussionCandidateSuggestions,
    json_output: bool,
) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(suggestions)?);
        return Ok(());
    }

    let rendered = serde_yaml::to_string(suggestions)?;
    if let Some(stripped) = rendered.strip_prefix("---\n") {
        print!("{stripped}");
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn build_candidate_prompt(comment: &core::Comment, thread: &DiscussionThread) -> String {
    let mut prompt = String::new();
    prompt.push_str("Selected review comment:\n");
    prompt.push_str(&format!(
        "- id: {}\n- file: {}\n- line: {}\n- rule_id: {}\n- severity: {}\n- category: {}\n- confidence: {:.0}%\n- comment: {}\n",
        comment.id,
        comment.file_path.display(),
        comment.line_number,
        comment.rule_id.as_deref().unwrap_or("<none>"),
        comment.severity,
        comment.category,
        comment.confidence * 100.0,
        comment.content,
    ));
    if let Some(suggestion) = &comment.suggestion {
        prompt.push_str(&format!("- suggested fix: {suggestion}\n"));
    }

    prompt.push_str("\nDiscussion thread:\n");
    if thread.turns.is_empty() {
        prompt.push_str("- no follow-up turns recorded\n");
    } else {
        for turn in &thread.turns {
            prompt.push_str(&format!("{}: {}\n", turn.role, turn.message));
        }
    }

    prompt.push_str(
        "\nTask:\n- Suggest at most 2 reusable rule candidates and at most 2 reusable custom_context entries.\n- A rule candidate should fit DiffScope rule files (id, description, optional scope, severity, category, tags).\n- A custom_context candidate should fit DiffScope config (optional scope, notes, files).\n- Only emit candidates when the guidance is durable across future reviews.\n- Reuse the existing rule_id when refining an obvious existing rule.\n- Keep rule ids lowercase and stable.\n- Use empty arrays when nothing should be promoted.\n",
    );

    prompt
}

fn discussion_candidate_response_schema() -> adapters::llm::StructuredOutputSchema {
    adapters::llm::StructuredOutputSchema::json_schema(
        "discussion_candidates",
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["summary", "rules", "custom_context"],
            "properties": {
                "summary": {"type": "string"},
                "rules": {
                    "type": "array",
                    "maxItems": MAX_DISCUSSION_RULE_CANDIDATES,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["id", "description", "scope", "severity", "category", "tags"],
                        "properties": {
                            "id": {"type": "string"},
                            "description": {"type": "string"},
                            "scope": {"type": ["string", "null"]},
                            "severity": {"type": ["string", "null"]},
                            "category": {"type": ["string", "null"]},
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"}
                            }
                        }
                    }
                },
                "custom_context": {
                    "type": "array",
                    "maxItems": MAX_DISCUSSION_CONTEXT_CANDIDATES,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["scope", "notes", "files"],
                        "properties": {
                            "scope": {"type": ["string", "null"]},
                            "notes": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "files": {
                                "type": "array",
                                "items": {"type": "string"}
                            }
                        }
                    }
                }
            }
        }),
    )
}

fn parse_candidate_response(content: &str) -> Result<RawDiscussionCandidateSuggestions> {
    let candidate = extract_json_candidate(content);
    serde_json::from_str(&candidate).with_context(|| {
        format!(
            "Failed to parse discussion candidate suggestions from model output: {}",
            content.trim()
        )
    })
}

fn extract_json_candidate(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("```") {
        trimmed
            .lines()
            .skip_while(|line| line.trim_start().starts_with("```"))
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        trimmed.to_string()
    }
}

fn normalize_candidate_suggestions(
    comment: &core::Comment,
    raw: RawDiscussionCandidateSuggestions,
) -> DiscussionCandidateSuggestions {
    let rules = raw
        .rules
        .into_iter()
        .filter_map(|candidate| normalize_rule_candidate(comment, candidate))
        .take(MAX_DISCUSSION_RULE_CANDIDATES)
        .collect::<Vec<_>>();
    let custom_context = raw
        .custom_context
        .into_iter()
        .filter_map(normalize_context_candidate)
        .take(MAX_DISCUSSION_CONTEXT_CANDIDATES)
        .collect::<Vec<_>>();

    let summary = raw.summary.trim().to_string();
    DiscussionCandidateSuggestions {
        summary: if summary.is_empty() {
            default_candidate_summary(rules.len(), custom_context.len())
        } else {
            summary
        },
        rules,
        custom_context,
    }
}

fn normalize_rule_candidate(
    comment: &core::Comment,
    mut candidate: DiscussionCandidateRule,
) -> Option<DiscussionCandidateRule> {
    candidate.id = sanitize_rule_id(&candidate.id);
    candidate.description = candidate.description.trim().to_string();
    if candidate.description.is_empty() {
        return None;
    }

    if candidate.id.is_empty() {
        candidate.id = comment
            .rule_id
            .as_deref()
            .map(sanitize_rule_id)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback_rule_id(comment, &candidate.description));
    }

    candidate.scope = normalize_optional_string(candidate.scope);
    candidate.severity = normalize_rule_attribute(
        candidate.severity,
        &["error", "warning", "info", "suggestion"],
    )
    .or_else(|| Some(comment.severity.as_str().to_string()));
    candidate.category = normalize_rule_attribute(
        candidate.category,
        &[
            "bug",
            "security",
            "performance",
            "style",
            "documentation",
            "bestpractice",
            "maintainability",
            "testing",
            "architecture",
        ],
    )
    .or_else(|| Some(comment.category.as_str().to_string()));

    candidate.tags = normalize_string_list(candidate.tags);
    if !candidate
        .tags
        .iter()
        .any(|tag| tag == DISCUSSION_DERIVED_TAG)
    {
        candidate.tags.push(DISCUSSION_DERIVED_TAG.to_string());
    }

    Some(candidate)
}

fn normalize_context_candidate(
    mut candidate: config::CustomContextConfig,
) -> Option<config::CustomContextConfig> {
    candidate.scope = normalize_optional_string(candidate.scope);
    candidate.notes = normalize_string_list(candidate.notes);
    candidate.files = normalize_string_list(candidate.files);

    if candidate.notes.is_empty() && candidate.files.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_rule_attribute(value: Option<String>, allowed: &[&str]) -> Option<String> {
    value.and_then(|value| {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() || !allowed.contains(&normalized.as_str()) {
            None
        } else {
            Some(normalized)
        }
    })
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && seen.insert(value.clone()))
        .collect()
}

fn sanitize_rule_id(value: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_separator = false;

    for ch in value.trim().chars() {
        let normalized = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            '.' | '_' | '-' => Some(ch),
            '/' | '\\' | ' ' => Some('.'),
            _ => None,
        };

        let Some(ch) = normalized else {
            continue;
        };

        let is_separator = matches!(ch, '.' | '_' | '-');
        if is_separator && (sanitized.is_empty() || previous_separator) {
            continue;
        }

        sanitized.push(ch);
        previous_separator = is_separator;
    }

    sanitized
        .trim_matches(|ch: char| matches!(ch, '.' | '_' | '-'))
        .to_string()
}

fn fallback_rule_id(comment: &core::Comment, description: &str) -> String {
    let slug = slugify_text(description);
    if slug.is_empty() {
        format!("discussion.{}", comment.category.as_str())
    } else {
        format!("discussion.{}.{}", comment.category.as_str(), slug)
    }
}

fn slugify_text(value: &str) -> String {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }

        if !current.is_empty() {
            parts.push(std::mem::take(&mut current));
            if parts.len() == 6 {
                break;
            }
        }
    }

    if !current.is_empty() && parts.len() < 6 {
        parts.push(current);
    }

    let mut slug = parts.join("-");
    if slug.len() > 48 {
        slug.truncate(48);
        slug = slug.trim_end_matches('-').to_string();
    }
    slug
}

fn default_candidate_summary(rule_count: usize, context_count: usize) -> String {
    format!(
        "Generated {} rule candidate{} and {} custom context snippet{} from the discussion thread.",
        rule_count,
        if rule_count == 1 { "" } else { "s" },
        context_count,
        if context_count == 1 { "" } else { "s" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::Mutex;

    struct FakeSuggestionAdapter {
        response: String,
        last_request: Mutex<Option<adapters::llm::LLMRequest>>,
    }

    impl FakeSuggestionAdapter {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
                last_request: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl adapters::llm::LLMAdapter for FakeSuggestionAdapter {
        async fn complete(
            &self,
            request: adapters::llm::LLMRequest,
        ) -> Result<adapters::llm::LLMResponse> {
            *self.last_request.lock().unwrap() = Some(request);
            Ok(adapters::llm::LLMResponse {
                content: self.response.clone(),
                model: "test-model".to_string(),
                usage: None,
            })
        }

        fn model_name(&self) -> &str {
            "test-model"
        }
    }

    fn make_comment() -> core::Comment {
        core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/auth/guard.rs"),
            line_number: 42,
            content: "Tenant boundary checks are missing on this privileged path.".to_string(),
            rule_id: Some("sec.auth.boundary".to_string()),
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::Security,
            suggestion: Some(
                "Validate the tenant context before accessing the account.".to_string(),
            ),
            confidence: 0.84,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Medium,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[tokio::test]
    async fn suggest_discussion_candidates_parses_and_normalizes_output() {
        let adapter = FakeSuggestionAdapter::new(
            r#"{
                "summary": "Human follow-up keeps reinforcing tenant boundary checks.",
                "rules": [
                    {
                        "id": "",
                        "description": "Require tenant boundary validation before privileged account access.",
                        "scope": " src/auth/** ",
                        "severity": "warning",
                        "category": null,
                        "tags": ["auth", "discussion-derived", "auth"]
                    },
                    {
                        "id": "bad",
                        "description": "   ",
                        "scope": null,
                        "severity": null,
                        "category": null,
                        "tags": []
                    }
                ],
                "custom_context": [
                    {
                        "scope": "src/auth/**",
                        "notes": ["Cross-check tenant isolation invariants before merging.", ""],
                        "files": ["docs/auth-boundaries.md", "docs/auth-boundaries.md"]
                    },
                    {
                        "scope": null,
                        "notes": [],
                        "files": []
                    }
                ]
            }"#,
        );

        let thread = DiscussionThread {
            comment_id: "comment-1".to_string(),
            turns: vec![
                super::super::types::DiscussionTurn {
                    role: "user".to_string(),
                    message: "We keep revisiting tenant-boundary bugs in auth reviews.".to_string(),
                },
                super::super::types::DiscussionTurn {
                    role: "assistant".to_string(),
                    message: "That sounds like durable guidance worth capturing.".to_string(),
                },
            ],
        };

        let suggestions = suggest_discussion_candidates(&adapter, &make_comment(), &thread)
            .await
            .unwrap();

        assert_eq!(suggestions.rules.len(), 1);
        assert_eq!(suggestions.rules[0].id, "sec.auth.boundary");
        assert_eq!(suggestions.rules[0].category.as_deref(), Some("security"));
        assert_eq!(suggestions.rules[0].severity.as_deref(), Some("warning"));
        assert_eq!(
            suggestions.rules[0].tags,
            vec!["auth".to_string(), "discussion-derived".to_string()]
        );
        assert_eq!(suggestions.custom_context.len(), 1);
        assert_eq!(
            suggestions.custom_context[0].files,
            vec!["docs/auth-boundaries.md".to_string()]
        );

        let request = adapter.last_request.lock().unwrap().clone().unwrap();
        assert!(request.response_schema.is_some());
        assert!(request.user_prompt.contains("tenant-boundary bugs"));
        assert!(request.user_prompt.contains("sec.auth.boundary"));
    }

    #[test]
    fn normalize_candidate_suggestions_adds_defaults_when_missing() {
        let comment = make_comment();
        let suggestions = normalize_candidate_suggestions(
            &comment,
            RawDiscussionCandidateSuggestions {
                summary: "".to_string(),
                rules: vec![DiscussionCandidateRule {
                    id: "  discussion auth rule  ".to_string(),
                    description: "Protect tenant boundaries on privileged reads".to_string(),
                    scope: None,
                    severity: None,
                    category: None,
                    tags: vec![],
                }],
                custom_context: vec![],
            },
        );

        assert!(suggestions.summary.contains("1 rule candidate"));
        assert_eq!(suggestions.rules[0].id, "discussion.auth.rule");
        assert_eq!(suggestions.rules[0].severity.as_deref(), Some("warning"));
        assert_eq!(suggestions.rules[0].category.as_deref(), Some("security"));
        assert_eq!(
            suggestions.rules[0].tags,
            vec!["discussion-derived".to_string()]
        );
    }
}
