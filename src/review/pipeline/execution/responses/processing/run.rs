use anyhow::Result;
use tracing::warn;

use crate::core;

use super::super::super::dispatcher::DispatchedJobResult;
use super::super::validation::validate_llm_response;
use super::comments::extract_processed_comments;
use super::fallback::fallback_result;
use super::merge::merge_processed_comments;
use super::usage::response_usage;
use super::{ProcessedJobResult, ResponseUsage};

pub(in super::super::super) fn process_job_result(
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

            let Some(comments) = extract_processed_comments(
                &response.content,
                diff,
                &active_rules,
                path_config.as_ref(),
                pass_kind,
            )?
            else {
                return Ok(fallback_result(
                    file_path,
                    deterministic_comments,
                    mark_file_complete,
                    latency_ms,
                    usage,
                    agent_data,
                ));
            };

            let comments = merge_processed_comments(diff, comments, deterministic_comments);

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
