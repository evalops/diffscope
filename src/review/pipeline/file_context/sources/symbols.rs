use anyhow::Result;

use crate::core;

use super::super::super::context::extract_symbols_from_diff;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;

pub(in super::super) async fn add_symbol_context(
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
