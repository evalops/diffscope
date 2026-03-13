#[path = "services/adapter_init.rs"]
mod adapter_init;
#[path = "services/run.rs"]
mod run;
#[path = "services/support.rs"]
mod support;

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters;
use crate::config;
use crate::core;
use crate::plugins;

use super::super::context_helpers::PatternRepositoryMap;
use super::super::feedback::FeedbackStore;
use run::build_pipeline_services;

pub(super) struct PipelineServices {
    pub config: config::Config,
    pub repo_path: PathBuf,
    pub context_fetcher: core::ContextFetcher,
    pub pattern_repositories: PatternRepositoryMap,
    pub review_rules: Vec<core::ReviewRule>,
    pub feedback: FeedbackStore,
    pub feedback_context: String,
    pub plugin_manager: plugins::plugin::PluginManager,
    pub adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub verification_adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub embedding_adapter: Option<Arc<dyn adapters::llm::LLMAdapter>>,
    pub base_prompt_config: core::prompt::PromptConfig,
    pub convention_store_path: Option<PathBuf>,
    pub is_local: bool,
}

impl PipelineServices {
    pub(super) async fn new(config: config::Config, repo_path: &Path) -> Result<Self> {
        build_pipeline_services(config, repo_path).await
    }

    pub(super) fn repo_path_str(&self) -> String {
        self.repo_path.to_string_lossy().to_string()
    }
}

pub(super) fn should_optimize_for_local(config: &config::Config) -> bool {
    adapter_init::should_optimize_for_local(config)
}
