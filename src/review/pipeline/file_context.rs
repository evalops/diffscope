use anyhow::Result;

#[path = "file_context/base.rs"]
mod base;
#[path = "file_context/finalize.rs"]
mod finalize;
#[path = "file_context/sources.rs"]
mod sources;

use crate::config;
use crate::core;

use super::context::extract_symbols_from_diff;
use super::services::PipelineServices;
use super::session::ReviewSession;

pub(super) struct PreparedFileContext {
    pub active_rules: Vec<core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub deterministic_comments: Vec<core::Comment>,
    pub context_chunks: Vec<core::LLMContextChunk>,
    pub graph_query_traces: Vec<core::dag::DagExecutionTrace>,
}

pub(super) async fn assemble_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    pre_analysis_context: Vec<core::LLMContextChunk>,
    deterministic_comments: Vec<core::Comment>,
) -> Result<PreparedFileContext> {
    let path_config = services.config.get_path_config(&diff.file_path).cloned();
    let mut context_chunks =
        base::initial_context_chunks(services, diff, pre_analysis_context).await?;
    let diff_symbols = extract_symbols_from_diff(diff);
    let mut graph_query_records = Vec::new();

    sources::add_symbol_context(
        services,
        session,
        diff,
        &diff_symbols,
        &mut context_chunks,
        &mut graph_query_records,
    )
    .await?;
    sources::add_related_file_context(
        services,
        session,
        diff,
        &mut context_chunks,
        &mut graph_query_records,
    );
    sources::add_semantic_context(
        services,
        session,
        diff,
        &diff_symbols,
        &mut context_chunks,
        &mut graph_query_records,
    )
    .await;
    sources::add_path_context(services, diff, path_config.as_ref(), &mut context_chunks).await?;
    sources::inject_repository_context(services, diff, &mut context_chunks).await?;

    if !diff_symbols.is_empty() && !graph_query_records.is_empty() {
        graph_query_records.insert(
            0,
            sources::trace_record(
                format!("seed_symbols={}", summarize_seed_symbols(&diff_symbols)),
                0,
            ),
        );
    }

    let graph_query_traces = sources::build_graph_query_trace(&diff.file_path, graph_query_records)
        .into_iter()
        .collect();

    Ok(finalize::finalize_file_context(
        services,
        diff,
        path_config,
        deterministic_comments,
        context_chunks,
        graph_query_traces,
    ))
}

fn summarize_seed_symbols(symbols: &[String]) -> String {
    let shown = symbols.iter().take(6).cloned().collect::<Vec<_>>();
    let mut summary = shown.join(", ");
    if symbols.len() > shown.len() {
        summary.push_str(&format!(" (+{} more)", symbols.len() - shown.len()));
    }
    summary
}
