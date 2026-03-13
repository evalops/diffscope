use std::sync::Arc;
use std::time::Instant;

use crate::adapters;
use crate::core;

use super::super::super::contracts::{FileReviewJob, ReviewExecutionContext};
use super::{AgentActivity, DispatchedJobResult};

pub(super) async fn dispatch_job(
    job: FileReviewJob,
    context: &ReviewExecutionContext<'_>,
    agent_ctx: Option<Arc<core::agent_tools::ReviewToolContext>>,
    loop_config: core::agent_loop::AgentLoopConfig,
    tools_filter: Option<Vec<String>>,
) -> DispatchedJobResult {
    let adapter = context.services.adapter.clone();

    if context.services.is_local {
        eprintln!("Sending {} to local model...", job.file_path.display());
    }
    let request_start = Instant::now();

    let (response, agent_data) = if let Some(ctx) = agent_ctx {
        let tools = core::agent_tools::build_review_tools(ctx, tools_filter.as_deref());
        let tool_defs: Vec<_> = tools.iter().map(|tool| tool.definition()).collect();
        let chat_request = adapters::llm::ChatRequest::from_llm_request(job.request, &tool_defs);
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
