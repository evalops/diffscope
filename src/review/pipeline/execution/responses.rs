use anyhow::Result;
use std::path::PathBuf;
use tracing::warn;

use crate::config;
use crate::core;
use crate::parsing::parse_llm_response;

use super::super::comments::filter_comments_for_diff;
use super::super::types::AgentActivity;
use super::dispatcher::DispatchedJobResult;

pub(super) struct ProcessedJobResult {
    pub file_path: PathBuf,
    pub comments: Vec<core::Comment>,
    pub latency_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub comment_count: usize,
    pub pass_tag: Option<String>,
    pub mark_file_complete: bool,
    pub agent_data: Option<AgentActivity>,
}

#[derive(Default)]
struct ResponseUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

pub(super) fn process_job_result(
    result: DispatchedJobResult,
    diff: &core::UnifiedDiff,
    is_local: bool,
) -> Result<ProcessedJobResult> {
    let DispatchedJobResult {
        active_rules,
        path_config,
        file_path,
        deterministic_comments,
        pass_kind,
        mark_file_complete,
        response,
        latency_ms,
        agent_data,
        ..
    } = result;

    match response {
        Err(error) => {
            warn!("LLM request failed for {}: {}", file_path.display(), error);
            Ok(fallback_result(
                file_path,
                deterministic_comments,
                mark_file_complete,
                latency_ms,
                ResponseUsage::default(),
                agent_data,
            ))
        }
        Ok(response) => {
            let usage = response_usage(&response);

            if let Err(validation_error) = validate_llm_response(&response.content) {
                eprintln!(
                    "Warning: LLM response validation failed for {}: {}",
                    file_path.display(),
                    validation_error
                );
                if is_local {
                    eprintln!(
                        "Hint: Try a larger model or reduce diff size for better results with local models."
                    );
                }
                return Ok(fallback_result(
                    file_path,
                    deterministic_comments,
                    mark_file_complete,
                    latency_ms,
                    usage,
                    agent_data,
                ));
            }

            let Ok(raw_comments) = parse_llm_response(&response.content, &diff.file_path) else {
                return Ok(fallback_result(
                    file_path,
                    deterministic_comments,
                    mark_file_complete,
                    latency_ms,
                    usage,
                    agent_data,
                ));
            };

            let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;
            apply_specialized_pass_tags(&mut comments, pass_kind);
            apply_path_severity_overrides(&mut comments, path_config.as_ref());

            let comments =
                super::super::super::rule_helpers::apply_rule_overrides(comments, &active_rules);

            let mut comments = filter_comments_for_diff(diff, comments);
            comments.extend(deterministic_comments);

            Ok(ProcessedJobResult {
                file_path,
                comment_count: comments.len(),
                comments,
                latency_ms,
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                pass_tag: Some(
                    pass_kind
                        .map(|kind| kind.tag().to_string())
                        .unwrap_or_else(|| "default".to_string()),
                ),
                mark_file_complete,
                agent_data,
            })
        }
    }
}

fn fallback_result(
    file_path: PathBuf,
    comments: Vec<core::Comment>,
    mark_file_complete: bool,
    latency_ms: u64,
    usage: ResponseUsage,
    agent_data: Option<AgentActivity>,
) -> ProcessedJobResult {
    let comment_count = comments.len();
    ProcessedJobResult {
        file_path,
        comments,
        latency_ms,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        comment_count,
        pass_tag: None,
        mark_file_complete,
        agent_data,
    }
}

fn response_usage(response: &crate::adapters::llm::LLMResponse) -> ResponseUsage {
    response
        .usage
        .as_ref()
        .map(|usage| ResponseUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        })
        .unwrap_or_default()
}

fn apply_specialized_pass_tags(
    comments: &mut [core::Comment],
    pass_kind: Option<core::SpecializedPassKind>,
) {
    let Some(pass_kind) = pass_kind else {
        return;
    };

    let tag = pass_kind.tag().to_string();
    for comment in comments {
        if !comment.tags.contains(&tag) {
            comment.tags.push(tag.clone());
        }
    }
}

fn apply_path_severity_overrides(
    comments: &mut [core::Comment],
    path_config: Option<&config::PathConfig>,
) {
    let Some(path_config) = path_config else {
        return;
    };

    for comment in comments {
        for (category, severity) in &path_config.severity_overrides {
            if comment.category.as_str() == category.to_lowercase() {
                if let Some(severity) = parse_path_severity_override(severity) {
                    comment.severity = severity;
                }
            }
        }
    }
}

fn parse_path_severity_override(value: &str) -> Option<core::comment::Severity> {
    match value.to_lowercase().as_str() {
        "error" => Some(core::comment::Severity::Error),
        "warning" => Some(core::comment::Severity::Warning),
        "info" => Some(core::comment::Severity::Info),
        "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

pub(super) fn validate_llm_response(response: &str) -> Result<(), String> {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return Err("Empty response from model".to_string());
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if is_structured_review_payload(&value) {
            return Ok(());
        }

        return Err("JSON response did not match the review output contract".to_string());
    }

    if response.len() < 10 {
        return Err("Response too short to contain valid review".to_string());
    }

    if has_excessive_repetition(response) {
        return Err("Response contains excessive repetition (model may be stuck)".to_string());
    }

    Ok(())
}

fn is_structured_review_payload(value: &serde_json::Value) -> bool {
    let items = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value
        .get("comments")
        .or_else(|| value.get("findings"))
        .or_else(|| value.get("results"))
        .and_then(|items| items.as_array())
    {
        array
    } else {
        return false;
    };

    items.iter().all(|item| {
        item.is_object()
            && (item.get("line").is_some()
                || item.get("line_number").is_some()
                || item.get("content").is_some()
                || item.get("issue").is_some())
    })
}

pub(super) fn has_excessive_repetition(text: &str) -> bool {
    if text.len() < 100 {
        return false;
    }
    let window = 20.min(text.len() / 5);
    let search_end = text.len().saturating_sub(window);
    for start in 0..search_end.max(1) {
        if !text.is_char_boundary(start) || !text.is_char_boundary(start + window) {
            continue;
        }
        let pattern = &text[start..start + window];
        if pattern.trim().is_empty() {
            continue;
        }
        if text.matches(pattern).count() > 5 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_response_accepts_valid_response() {
        let response = "Here is my review of the code changes:\n- Line 5: potential null reference";
        assert!(validate_llm_response(response).is_ok());
    }

    #[test]
    fn validate_response_accepts_structured_json() {
        assert!(validate_llm_response("[]").is_ok());
        assert!(validate_llm_response("[{\"line\":10,\"issue\":\"problem\"}]").is_ok());
    }

    #[test]
    fn validate_response_rejects_empty() {
        assert!(validate_llm_response("").is_err());
        assert!(validate_llm_response("   \n\t  ").is_err());
    }

    #[test]
    fn validate_response_rejects_too_short() {
        assert!(validate_llm_response("OK").is_err());
        assert!(validate_llm_response("no issue").is_err());
    }

    #[test]
    fn validate_response_rejects_repetitive() {
        let repeated = "This is a repeating segment.".repeat(20);
        assert!(validate_llm_response(&repeated).is_err());
    }

    #[test]
    fn repetition_short_text_always_false() {
        assert!(!has_excessive_repetition("short"));
        assert!(!has_excessive_repetition(""));
        assert!(!has_excessive_repetition("a".repeat(99).as_str()));
    }

    #[test]
    fn repetition_normal_text_false() {
        let text = "This is a normal code review response. The function looks correct \
                    but there may be an edge case on line 42 where the input could be null. \
                    Consider adding a guard clause to handle this scenario.";
        assert!(!has_excessive_repetition(text));
    }

    #[test]
    fn repetition_stuck_model_detected() {
        let text = "The code looks fine. ".repeat(10);
        assert!(has_excessive_repetition(&text));
    }

    #[test]
    fn repetition_whitespace_only_not_flagged() {
        let text = " ".repeat(200);
        assert!(!has_excessive_repetition(&text));
    }

    #[test]
    fn test_has_excessive_repetition_boundary_120_chars() {
        let pattern = "abcdefghij1234567890";
        let text = pattern.repeat(6);
        assert_eq!(text.len(), 120);
        assert!(has_excessive_repetition(&text));
    }

    #[test]
    fn test_has_excessive_repetition_short_not_detected() {
        let text = "abc".repeat(30);
        assert!(!has_excessive_repetition(&text));
    }
}
