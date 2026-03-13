use anyhow::Result;
use std::path::PathBuf;
use tracing::warn;

use crate::adapters::llm::LLMResponse;
use crate::core;
use crate::parsing::parse_llm_response;
use crate::review::{apply_rule_overrides, filter_comments_for_diff, AgentActivity};

use super::super::dispatcher::DispatchedJobResult;
use super::overrides::{apply_path_severity_overrides, apply_specialized_pass_tags};
use super::validation::validate_llm_response;

pub(in super::super) struct ProcessedJobResult {
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

pub(in super::super) fn process_job_result(
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

            let comments = apply_rule_overrides(comments, &active_rules);

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

fn response_usage(response: &LLMResponse) -> ResponseUsage {
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
