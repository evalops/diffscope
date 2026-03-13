use std::sync::Arc;

use crate::core;

use super::super::super::contracts::ReviewExecutionContext;

const MAX_CONCURRENT_FILES: usize = 5;

pub(super) fn dispatch_concurrency(context: &ReviewExecutionContext<'_>) -> usize {
    if context.services.is_local {
        1
    } else {
        MAX_CONCURRENT_FILES
    }
}

pub(super) fn build_agent_loop_config(
    context: &ReviewExecutionContext<'_>,
) -> core::agent_loop::AgentLoopConfig {
    core::agent_loop::AgentLoopConfig {
        max_iterations: context.services.config.agent_max_iterations,
        max_total_tokens: context.services.config.agent_max_total_tokens,
    }
}

pub(super) fn build_agent_tool_context(
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
