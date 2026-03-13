use crate::core;

use super::super::super::context::gather_related_file_context;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;

pub(in super::super) fn add_related_file_context(
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
