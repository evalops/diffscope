use futures::StreamExt;

use super::super::super::contracts::{FileReviewJob, ReviewExecutionContext};
use super::context::{build_agent_loop_config, build_agent_tool_context, dispatch_concurrency};
use super::job::dispatch_job;
use super::DispatchedJobResult;

pub(in super::super) async fn dispatch_jobs(
    jobs: Vec<FileReviewJob>,
    context: &ReviewExecutionContext<'_>,
) -> Vec<DispatchedJobResult> {
    let concurrency = dispatch_concurrency(context);
    tracing::info!(
        "Sending {} LLM requests (concurrency={})",
        jobs.len(),
        concurrency,
    );

    let agent_tool_ctx = build_agent_tool_context(context);
    let agent_loop_config = build_agent_loop_config(context);
    let agent_tools_filter = context.services.config.agent.tools_enabled.clone();

    futures::stream::iter(jobs)
        .map(|job| {
            let agent_ctx = agent_tool_ctx.clone();
            let loop_config = agent_loop_config.clone();
            let tools_filter = agent_tools_filter.clone();
            async move { dispatch_job(job, context, agent_ctx, loop_config, tools_filter).await }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await
}
