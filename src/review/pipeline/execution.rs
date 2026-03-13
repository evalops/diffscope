use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use crate::adapters;
use crate::config;
use crate::core;
use crate::parsing::parse_llm_response;

use super::helpers::{filter_comments_for_diff, merge_file_metric, validate_llm_response};
use super::{AgentActivity, FileMetric, ProgressCallback, ProgressUpdate};

pub(super) struct FileReviewJob {
    pub job_order: usize,
    pub diff_index: usize,
    pub request: adapters::llm::LLMRequest,
    pub active_rules: Vec<crate::core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub file_path: std::path::PathBuf,
    pub deterministic_comments: Vec<core::Comment>,
    pub pass_kind: Option<core::SpecializedPassKind>,
    pub mark_file_complete: bool,
}

pub(super) struct ReviewExecutionContext<'a> {
    pub diffs: &'a [core::UnifiedDiff],
    pub config: &'a config::Config,
    pub repo_path: &'a Path,
    pub adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub is_local: bool,
    pub on_progress: Option<ProgressCallback>,
    pub initial_comments: Vec<core::Comment>,
    pub files_total: usize,
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
    let concurrency = if context.is_local {
        1
    } else {
        MAX_CONCURRENT_FILES
    };

    tracing::info!(
        "Sending {} LLM requests (concurrency={})",
        jobs.len(),
        concurrency,
    );

    let agent_tool_ctx = if context.config.agent_review && context.adapter.supports_tools() {
        let context_fetcher_arc =
            Arc::new(core::ContextFetcher::new(context.repo_path.to_path_buf()));
        Some(Arc::new(core::agent_tools::ReviewToolContext {
            repo_path: context.repo_path.to_path_buf(),
            context_fetcher: context_fetcher_arc,
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        }))
    } else {
        None
    };
    let agent_loop_config = core::agent_loop::AgentLoopConfig {
        max_iterations: context.config.agent_max_iterations,
        max_total_tokens: context.config.agent_max_total_tokens,
    };
    let agent_tools_filter = context.config.agent_tools_enabled.clone();
    let files_skipped_snapshot = context.files_skipped;

    let results: Vec<_> = futures::stream::iter(jobs)
        .map(|job| {
            let adapter = context.adapter.clone();
            let agent_ctx = agent_tool_ctx.clone();
            let loop_config = agent_loop_config.clone();
            let tools_filter = agent_tools_filter.clone();
            async move {
                if context.is_local {
                    eprintln!("Sending {} to local model...", job.file_path.display());
                }
                let request_start = Instant::now();

                let (response, agent_data) = if let Some(ctx) = agent_ctx {
                    let tools = core::agent_tools::build_review_tools(ctx, tools_filter.as_deref());
                    let tool_defs: Vec<_> = tools.iter().map(|t| t.definition()).collect();
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
                        Err(e) => (Err(e), None),
                    }
                } else {
                    (adapter.complete(job.request).await, None)
                };

                let latency_ms = request_start.elapsed().as_millis() as u64;
                if context.is_local {
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
    let mut total_prompt_tokens: usize = 0;
    let mut total_completion_tokens: usize = 0;
    let mut total_tokens: usize = 0;
    let mut file_metrics: Vec<FileMetric> = Vec::new();
    let mut comments_by_pass: HashMap<String, usize> = HashMap::new();
    let mut aggregate_agent_iterations: usize = 0;
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
        let diff = &context.diffs[diff_index];

        match response {
            Err(e) => {
                warn!("LLM request failed for {}: {}", file_path.display(), e);
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

                if let Err(validation_err) = validate_llm_response(&response.content) {
                    eprintln!(
                        "Warning: LLM response validation failed for {}: {}",
                        file_path.display(),
                        validation_err
                    );
                    if context.is_local {
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

                    if let Some(ref pc) = path_config {
                        for comment in &mut comments {
                            for (category, severity) in &pc.severity_overrides {
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
                        .map(|k| k.tag().to_string())
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
            if let Some(ref cb) = context.on_progress {
                cb(ProgressUpdate {
                    current_file: file_path.display().to_string(),
                    files_total: context.files_total,
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
