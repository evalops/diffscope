use anyhow::Result;

use crate::config;
use crate::core;

use super::super::context::{extract_symbols_from_diff, gather_related_file_context};
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;

pub(super) async fn add_symbol_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let symbols = extract_symbols_from_diff(diff);
    if symbols.is_empty() {
        return Ok(());
    }

    let definition_chunks = services
        .context_fetcher
        .fetch_related_definitions(&diff.file_path, &symbols)
        .await?;
    context_chunks.extend(definition_chunks);

    if let Some(index) = session.symbol_index.as_ref() {
        let index_chunks = services
            .context_fetcher
            .fetch_related_definitions_with_index(
                &diff.file_path,
                &symbols,
                index,
                services.config.symbol_index_max_locations,
                services.config.symbol_index_graph_hops,
                services.config.symbol_index_graph_max_files,
            )
            .await?;
        context_chunks.extend(index_chunks);
    }

    Ok(())
}

pub(super) fn add_related_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    if let Some(index) = session.symbol_index.as_ref() {
        let caller_chunks =
            gather_related_file_context(index, &diff.file_path, &services.repo_path);
        context_chunks.extend(caller_chunks);
    }
}

pub(super) async fn add_semantic_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    let Some(index) = session.semantic_index.as_ref() else {
        return;
    };

    let semantic_chunks = core::semantic_context_for_diff(
        index,
        diff,
        session
            .source_files
            .get(&diff.file_path)
            .map(|content| content.as_str()),
        services.embedding_adapter.as_deref(),
        services.config.semantic_rag_top_k,
        services.config.semantic_rag_min_similarity,
    )
    .await;
    context_chunks.extend(semantic_chunks);
}

pub(super) async fn add_path_context(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    path_config: Option<&config::PathConfig>,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let Some(path_config) = path_config else {
        return Ok(());
    };

    if !path_config.focus.is_empty() {
        context_chunks.push(
            core::LLMContextChunk::documentation(
                diff.file_path.clone(),
                format!(
                    "Focus areas for this file: {}",
                    path_config.focus.join(", ")
                ),
            )
            .with_provenance(core::ContextProvenance::PathSpecificFocusAreas),
        );
    }

    if !path_config.extra_context.is_empty() {
        let extra_chunks = services
            .context_fetcher
            .fetch_additional_context(&path_config.extra_context)
            .await?;
        context_chunks.extend(extra_chunks);
    }

    Ok(())
}

pub(super) async fn inject_repository_context(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    super::super::super::context_helpers::inject_custom_context(
        &services.config,
        &services.context_fetcher,
        diff,
        context_chunks,
    )
    .await?;
    super::super::super::context_helpers::inject_pattern_repository_context(
        &services.config,
        &services.pattern_repositories,
        &services.context_fetcher,
        diff,
        context_chunks,
    )
    .await?;

    Ok(())
}
