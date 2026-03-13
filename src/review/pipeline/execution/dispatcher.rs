use anyhow::Result;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::adapters;
use crate::core;

use super::super::contracts::{FileReviewJob, ReviewExecutionContext};
use super::super::types::AgentActivity;

pub(super) struct DispatchedJobResult {
    pub job_order: usize,
    pub diff_index: usize,
    pub active_rules: Vec<crate::core::ReviewRule>,
    pub path_config: Option<crate::config::PathConfig>,
    pub file_path: PathBuf,
    pub deterministic_comments: Vec<core::Comment>,
    pub pass_kind: Option<core::SpecializedPassKind>,
    pub mark_file_complete: bool,
    pub response: Result<adapters::llm::LLMResponse>,
    pub latency_ms: u64,
    pub agent_data: Option<AgentActivity>,
}

pub(super) async fn dispatch_jobs(
    jobs: Vec<FileReviewJob>,
    context: &ReviewExecutionContext<'_>,
) -> Vec<DispatchedJobResult> {
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

    let agent_tool_ctx = build_agent_tool_context(context);
    let agent_loop_config = core::agent_loop::AgentLoopConfig {
        max_iterations: context.services.config.agent_max_iterations,
        max_total_tokens: context.services.config.agent_max_total_tokens,
    };
    let agent_tools_filter = context.services.config.agent_tools_enabled.clone();

    futures::stream::iter(jobs)
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
                DispatchedJobResult {
                    job_order: job.job_order,
                    diff_index: job.diff_index,
                    active_rules: job.active_rules,
                    path_config: job.path_config,
                    file_path: job.file_path,
                    deterministic_comments: job.deterministic_comments,
                    pass_kind: job.pass_kind,
                    mark_file_complete: job.mark_file_complete,
                    response,
                    latency_ms,
                    agent_data,
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await
}

fn build_agent_tool_context(
    context: &ReviewExecutionContext<'_>,
) -> Option<Arc<core::agent_tools::ReviewToolContext>> {
    if !(context.services.config.agent_review && context.services.adapter.supports_tools()) {
        return None;
    }

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
}
