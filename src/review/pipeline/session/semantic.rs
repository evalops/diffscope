use tracing::warn;

use crate::core;

use super::super::services::PipelineServices;

pub(super) async fn build_semantic_index(
    diffs: &[core::UnifiedDiff],
    services: &PipelineServices,
) -> Option<core::semantic::SemanticIndex> {
    if !services.config.semantic_rag {
        return None;
    }

    let index_path = core::default_index_path(&services.repo_path);
    let changed_files = diffs
        .iter()
        .map(|diff| diff.file_path.clone())
        .collect::<Vec<_>>();

    match core::refresh_semantic_index(
        &services.repo_path,
        &index_path,
        services.embedding_adapter.as_deref(),
        &changed_files,
        |path| services.config.should_exclude(path),
        services.config.semantic_rag_max_files,
    )
    .await
    {
        Ok(index) => Some(index),
        Err(error) => {
            warn!("Semantic index refresh failed: {}", error);
            None
        }
    }
}

pub(super) fn load_semantic_feedback_store(
    services: &PipelineServices,
) -> Option<core::SemanticFeedbackStore> {
    if !services.config.semantic_feedback {
        return None;
    }

    let path = core::default_semantic_feedback_path(&services.config.feedback_path);
    let mut store = core::load_semantic_feedback_store(&path);
    core::align_semantic_feedback_store(&mut store, services.embedding_adapter.as_deref());
    Some(store)
}
