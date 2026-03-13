use anyhow::Result;

#[path = "file_context/base.rs"]
mod base;
#[path = "file_context/finalize.rs"]
mod finalize;
#[path = "file_context/sources.rs"]
mod sources;

use crate::config;
use crate::core;

use super::services::PipelineServices;
use super::session::ReviewSession;

pub(super) struct PreparedFileContext {
    pub active_rules: Vec<core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub deterministic_comments: Vec<core::Comment>,
    pub context_chunks: Vec<core::LLMContextChunk>,
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

    sources::add_symbol_context(services, session, diff, &mut context_chunks).await?;
    sources::add_related_file_context(services, session, diff, &mut context_chunks);
    sources::add_semantic_context(services, session, diff, &mut context_chunks).await;
    sources::add_path_context(services, diff, path_config.as_ref(), &mut context_chunks).await?;
    sources::inject_repository_context(services, diff, &mut context_chunks).await?;

    Ok(finalize::finalize_file_context(
        services,
        diff,
        path_config,
        deterministic_comments,
        context_chunks,
    ))
}
