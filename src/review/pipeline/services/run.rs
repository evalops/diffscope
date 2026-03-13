use anyhow::Result;
use std::path::Path;

use crate::core;

use super::adapter_init::build_adapter_services;
use super::support::build_support_services;
use super::PipelineServices;

pub(super) async fn build_pipeline_services(
    config: crate::config::Config,
    repo_path: &Path,
) -> Result<PipelineServices> {
    let repo_path = repo_path.to_path_buf();
    let adapter_services = build_adapter_services(&config)?;
    let support_services = build_support_services(&config, &repo_path).await?;

    Ok(PipelineServices {
        config,
        repo_path: repo_path.clone(),
        context_fetcher: core::ContextFetcher::new(repo_path),
        pattern_repositories: support_services.pattern_repositories,
        review_rules: support_services.review_rules,
        feedback: support_services.feedback,
        feedback_context: support_services.feedback_context,
        plugin_manager: support_services.plugin_manager,
        adapter: adapter_services.adapter,
        verification_adapter: adapter_services.verification_adapter,
        embedding_adapter: adapter_services.embedding_adapter,
        base_prompt_config: support_services.base_prompt_config,
        convention_store_path: support_services.convention_store_path,
        is_local: adapter_services.is_local,
    })
}
