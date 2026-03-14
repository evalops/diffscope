use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;

use crate::config;
use crate::core;

use super::super::super::context::extract_symbols_from_diff;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;

pub(in super::super) async fn add_semantic_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    let Some(index) = session.semantic_index.as_ref() else {
        return;
    };
    let preferred_files = graph_ranked_semantic_files(services, session, diff);

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
        &preferred_files,
    )
    .await;
    context_chunks.extend(semantic_chunks);
}

fn graph_ranked_semantic_files(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
) -> Vec<PathBuf> {
    let Some(index) = session.symbol_index.as_ref() else {
        return Vec::new();
    };

    let symbols = extract_symbols_from_diff(diff);
    if symbols.is_empty() {
        return Vec::new();
    }

    let retriever = core::SymbolContextRetriever::new(
        index,
        core::SymbolRetrievalPolicy::new(
            services.config.symbol_index_max_locations,
            services.config.symbol_index_graph_hops,
            services.config.symbol_index_graph_max_files,
        ),
    );
    let related_locations = retriever.related_symbol_locations(&diff.file_path, &symbols);

    let mut preferred_files = Vec::new();
    let mut seen = HashSet::new();

    for location in related_locations.definition_locations {
        if location.file_path == diff.file_path || !seen.insert(location.file_path.clone()) {
            continue;
        }
        preferred_files.push(location.file_path);
    }

    let mut reference_files = related_locations
        .reference_locations
        .into_iter()
        .map(|location| location.file_path)
        .filter(|file_path| file_path != &diff.file_path)
        .filter(|file_path| seen.insert(file_path.clone()))
        .collect::<Vec<_>>();
    reference_files.sort();
    preferred_files.extend(reference_files);

    preferred_files
}

pub(in super::super) async fn add_path_context(
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
