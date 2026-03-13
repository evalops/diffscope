use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use crate::adapters;
use crate::core;
use crate::parsing::parse_llm_response;

use super::comments::filter_comments_for_diff;
use super::session::{PipelineServices, ReviewSession};
use super::types::{AgentActivity, FileMetric, ProgressUpdate};

pub(super) struct FileReviewJob {
    pub job_order: usize,
    pub diff_index: usize,
    pub request: adapters::llm::LLMRequest,
    pub active_rules: Vec<crate::core::ReviewRule>,
    pub path_config: Option<crate::config::PathConfig>,
    pub file_path: PathBuf,
    pub deterministic_comments: Vec<core::Comment>,
    pub pass_kind: Option<core::SpecializedPassKind>,
    pub mark_file_complete: bool,
}

pub(super) struct ReviewExecutionContext<'a> {
    pub services: &'a PipelineServices,
    pub session: &'a ReviewSession,
    pub initial_comments: Vec<core::Comment>,
    pub files_completed: usize,
    pub files_skipped: usize,
}

pub(super) struct ExecutionSummary {
    pub all_comments: Vec<core::Comment>,
    pub total_prompt_tokens: usize,
    pub total_completion_tokens: usize,
    pub total_tokens: usize,
    pub file_metrics: Vec<FileMetric>,
    pub comments_by_pass: HashMap<String, usize>,
    pub agent_activity: Option<AgentActivity>,
}

pub(super) async fn execute_review_jobs(
    jobs: Vec<FileReviewJob>,
    context: ReviewExecutionContext<'_>,
) -> Result<ExecutionSummary> {
    const MAX_CONCURRENT_FILES: usize = 5;
    let concurrency = if context.services.is_local {
        1
    } else {
        MAX_CONCURRENT_FILES
    };

    tracing::info!(
        "Sending {} LLM requests (concurrency={})",
        jobs.len(),
        concurrency,
    );

    let agent_tool_ctx =
        if context.services.config.agent_review && context.services.adapter.supports_tools() {
            let context_fetcher_arc = Arc::new(core::ContextFetcher::new(
                context.services.repo_path.clone(),
            ));
            Some(Arc::new(core::agent_tools::ReviewToolContext {
                repo_path: context.services.repo_path.clone(),
                context_fetcher: context_fetcher_arc,
                symbol_index: None,
                symbol_graph: None,
                git_history: None,
            }))
        } else {
            None
        };
    let agent_loop_config = core::agent_loop::AgentLoopConfig {
        max_iterations: context.services.config.agent_max_iterations,
        max_total_tokens: context.services.config.agent_max_total_tokens,
    };
    let agent_tools_filter = context.services.config.agent_tools_enabled.clone();
    let files_skipped_snapshot = context.files_skipped;

    let results: Vec<_> = futures::stream::iter(jobs)
        .map(|job| {
            let adapter = context.services.adapter.clone();
            let agent_ctx = agent_tool_ctx.clone();
            let loop_config = agent_loop_config.clone();
            let tools_filter = agent_tools_filter.clone();
            async move {
                if context.services.is_local {
                    eprintln!("Sending {} to local model...", job.file_path.display());
                }
                let request_start = Instant::now();

                let (response, agent_data) = if let Some(ctx) = agent_ctx {
                    let tools = core::agent_tools::build_review_tools(ctx, tools_filter.as_deref());
                    let tool_defs: Vec<_> = tools.iter().map(|tool| tool.definition()).collect();
                    let chat_request =
                        adapters::llm::ChatRequest::from_llm_request(job.request, &tool_defs);
                    match core::agent_loop::run_agent_loop(
                        adapter.as_ref(),
                        chat_request,
                        &tools,
                        &loop_config,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            let activity = AgentActivity {
                                total_iterations: result.iterations,
                                tool_calls: result.tool_calls,
                            };
                            (
                                Ok(adapters::llm::LLMResponse {
                                    content: result.content,
                                    model: result.model,
                                    usage: Some(result.total_usage),
                                }),
                                Some(activity),
                            )
                        }
                        Err(error) => (Err(error), None),
                    }
                } else {
                    (adapter.complete(job.request).await, None)
                };

                let latency_ms = request_start.elapsed().as_millis() as u64;
                if context.services.is_local {
                    eprintln!(
                        "{}: response received ({:.1}s)",
                        job.file_path.display(),
                        latency_ms as f64 / 1000.0
                    );
                }
                (
                    job.job_order,
                    job.diff_index,
                    job.active_rules,
                    job.path_config,
                    job.file_path,
                    job.deterministic_comments,
                    job.pass_kind,
                    job.mark_file_complete,
                    response,
                    latency_ms,
                    agent_data,
                )
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let mut indexed_results = results;
    indexed_results.sort_by_key(|(job_order, _, _, _, _, _, _, _, _, _, _)| *job_order);

    let mut all_comments = context.initial_comments;
    let mut files_completed = context.files_completed;
    let mut total_prompt_tokens = 0usize;
    let mut total_completion_tokens = 0usize;
    let mut total_tokens = 0usize;
    let mut file_metrics = Vec::new();
    let mut comments_by_pass: HashMap<String, usize> = HashMap::new();
    let mut aggregate_agent_iterations = 0usize;
    let mut aggregate_agent_tool_calls: Vec<core::agent_loop::AgentToolCallLog> = Vec::new();
    let mut has_agent_activity = false;

    for (
        _job_order,
        diff_index,
        active_rules,
        path_config,
        file_path,
        deterministic_comments,
        pass_kind,
        mark_file_complete,
        response,
        latency_ms,
        agent_data,
    ) in indexed_results
    {
        let diff = &context.session.diffs[diff_index];

        match response {
            Err(error) => {
                warn!("LLM request failed for {}: {}", file_path.display(), error);
                merge_file_metric(
                    &mut file_metrics,
                    &file_path,
                    latency_ms,
                    0,
                    0,
                    0,
                    deterministic_comments.len(),
                );
                all_comments.extend(deterministic_comments);
            }
            Ok(response) => {
                let (resp_prompt_tokens, resp_completion_tokens, resp_total_tokens) =
                    if let Some(ref usage) = response.usage {
                        (
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens,
                        )
                    } else {
                        (0, 0, 0)
                    };
                total_prompt_tokens += resp_prompt_tokens;
                total_completion_tokens += resp_completion_tokens;
                total_tokens += resp_total_tokens;

                if let Some(activity) = agent_data {
                    has_agent_activity = true;
                    aggregate_agent_iterations += activity.total_iterations;
                    aggregate_agent_tool_calls.extend(activity.tool_calls);
                }

                if let Err(validation_error) = validate_llm_response(&response.content) {
                    eprintln!(
                        "Warning: LLM response validation failed for {}: {}",
                        file_path.display(),
                        validation_error
                    );
                    if context.services.is_local {
                        eprintln!(
                            "Hint: Try a larger model or reduce diff size for better results with local models."
                        );
                    }
                    let analyzer_comment_count = deterministic_comments.len();
                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        analyzer_comment_count,
                    );
                    all_comments.extend(deterministic_comments);
                    continue;
                }

                if let Ok(raw_comments) = parse_llm_response(&response.content, &diff.file_path) {
                    let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;

                    if let Some(kind) = pass_kind {
                        for comment in &mut comments {
                            if !comment.tags.contains(&kind.tag().to_string()) {
                                comment.tags.push(kind.tag().to_string());
                            }
                        }
                    }

                    if let Some(ref path_config) = path_config {
                        for comment in &mut comments {
                            for (category, severity) in &path_config.severity_overrides {
                                if comment.category.as_str() == category.to_lowercase() {
                                    comment.severity = match severity.to_lowercase().as_str() {
                                        "error" => core::comment::Severity::Error,
                                        "warning" => core::comment::Severity::Warning,
                                        "info" => core::comment::Severity::Info,
                                        "suggestion" => core::comment::Severity::Suggestion,
                                        _ => comment.severity.clone(),
                                    };
                                }
                            }
                        }
                    }
                    let comments =
                        super::super::rule_helpers::apply_rule_overrides(comments, &active_rules);

                    let mut comments = filter_comments_for_diff(diff, comments);
                    comments.extend(deterministic_comments);
                    let comment_count = comments.len();

                    let pass_tag = pass_kind
                        .map(|kind| kind.tag().to_string())
                        .unwrap_or_else(|| "default".to_string());
                    *comments_by_pass.entry(pass_tag).or_insert(0) += comment_count;

                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        comment_count,
                    );

                    all_comments.extend(comments);
                } else {
                    let analyzer_comment_count = deterministic_comments.len();
                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        analyzer_comment_count,
                    );
                    all_comments.extend(deterministic_comments);
                }
            }
        }

        if mark_file_complete {
            files_completed += 1;
            if let Some(ref callback) = context.session.on_progress {
                callback(ProgressUpdate {
                    current_file: file_path.display().to_string(),
                    files_total: context.session.files_total,
                    files_completed,
                    files_skipped: files_skipped_snapshot,
                    comments_so_far: all_comments.clone(),
                });
            }
        }
    }

    Ok(ExecutionSummary {
        all_comments,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        file_metrics,
        comments_by_pass,
        agent_activity: if has_agent_activity {
            Some(AgentActivity {
                total_iterations: aggregate_agent_iterations,
                tool_calls: aggregate_agent_tool_calls,
            })
        } else {
            None
        },
    })
}

fn validate_llm_response(response: &str) -> Result<(), String> {
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

fn has_excessive_repetition(text: &str) -> bool {
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

fn merge_file_metric(
    file_metrics: &mut Vec<FileMetric>,
    file_path: &Path,
    latency_ms: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
    comment_count: usize,
) {
    if let Some(existing) = file_metrics
        .iter_mut()
        .find(|metric| metric.file_path == file_path)
    {
        existing.prompt_tokens += prompt_tokens;
        existing.completion_tokens += completion_tokens;
        existing.total_tokens += total_tokens;
        existing.comment_count += comment_count;
        if latency_ms > existing.latency_ms {
            existing.latency_ms = latency_ms;
        }
        return;
    }

    file_metrics.push(FileMetric {
        file_path: file_path.to_path_buf(),
        latency_ms,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        comment_count,
    });
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
