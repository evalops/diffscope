use anyhow::Result;

use crate::core;

use super::super::super::services::PipelineServices;

pub(in super::super) async fn inject_repository_context(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    super::super::super::super::context_helpers::inject_linked_issue_context(
        &services.config,
        diff,
        context_chunks,
    );
    super::super::super::super::context_helpers::inject_document_context(
        &services.config,
        diff,
        context_chunks,
    );
    super::super::super::super::context_helpers::inject_custom_context(
        &services.config,
        &services.context_fetcher,
        diff,
        context_chunks,
    )
    .await?;
    super::super::super::super::context_helpers::inject_pattern_repository_context(
        &services.config,
        &services.pattern_repositories,
        &services.context_fetcher,
        diff,
        context_chunks,
    )
    .await?;

    Ok(())
}
