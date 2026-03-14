use crate::config;
use crate::core;

use super::super::services::PipelineServices;
use super::PreparedFileContext;

pub(super) fn finalize_file_context(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    path_config: Option<config::PathConfig>,
    deterministic_comments: Vec<core::Comment>,
    mut context_chunks: Vec<core::LLMContextChunk>,
    graph_query_traces: Vec<core::dag::DagExecutionTrace>,
) -> PreparedFileContext {
    let active_rules = core::active_rules_for_file(
        &services.review_rules,
        &diff.file_path,
        services.config.max_active_rules,
    );
    super::super::super::rule_helpers::inject_rule_context(
        diff,
        &active_rules,
        &mut context_chunks,
    );
    context_chunks = super::super::super::context_helpers::rank_and_trim_context_chunks(
        diff,
        context_chunks,
        services.config.context_max_chunks,
        services.config.context_budget_chars,
    );

    PreparedFileContext {
        active_rules,
        path_config,
        deterministic_comments,
        context_chunks,
        graph_query_traces,
    }
}
