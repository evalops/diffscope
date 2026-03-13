use anyhow::Result;

use crate::config;
use crate::core;

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
