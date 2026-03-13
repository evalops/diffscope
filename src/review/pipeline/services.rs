use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::plugins;

use super::super::context_helpers::{resolve_pattern_repositories, PatternRepositoryMap};
use super::super::feedback::{generate_feedback_context, load_feedback_store, FeedbackStore};
use super::super::rule_helpers::load_review_rules;
use super::repo_support::resolve_convention_store_path;

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
        let repo_path = repo_path.to_path_buf();
        let is_local = should_optimize_for_local(&config);
        let convention_store_path = resolve_convention_store_path(&config);
        let pattern_repositories = resolve_pattern_repositories(&config, &repo_path);
        let review_rules = load_review_rules(&config, &pattern_repositories, &repo_path);

        let mut plugin_manager = plugins::plugin::PluginManager::new();
        plugin_manager.load_builtin_plugins(&config.plugins).await?;

        let feedback = load_feedback_store(&config);
        let feedback_context = if config.enhanced_feedback {
            generate_feedback_context(&feedback)
        } else {
            String::new()
        };

        let model_config = config.to_model_config();
        let adapter: Arc<dyn adapters::llm::LLMAdapter> =
            Arc::from(adapters::llm::create_adapter(&model_config)?);
        info!("Review adapter: {}", adapter.model_name());

        let verification_adapter: Arc<dyn adapters::llm::LLMAdapter> = {
            let verification_config =
                config.to_model_config_for_role(config.verification_model_role);
            if verification_config.model_name != model_config.model_name {
                info!(
                    "Using '{}' model '{}' for verification pass",
                    format!("{:?}", config.verification_model_role).to_lowercase(),
                    verification_config.model_name
                );
                Arc::from(adapters::llm::create_adapter(&verification_config)?)
            } else {
                adapter.clone()
            }
        };

        let embedding_adapter = if config.semantic_rag || config.semantic_feedback {
            let embedding_config = config.to_model_config_for_role(config::ModelRole::Embedding);
            if embedding_config.model_name == model_config.model_name {
                Some(adapter.clone())
            } else {
                match adapters::llm::create_adapter(&embedding_config) {
                    Ok(adapter) => Some(Arc::from(adapter)),
                    Err(error) => {
                        warn!(
                            "Embedding adapter initialization failed for '{}': {}",
                            embedding_config.model_name, error
                        );
                        None
                    }
                }
            }
        } else {
            None
        };

        let base_prompt_config = core::prompt::PromptConfig {
            max_context_chars: config.max_context_chars,
            max_diff_chars: config.max_diff_chars,
            ..Default::default()
        };

        Ok(Self {
            config,
            repo_path: repo_path.clone(),
            context_fetcher: core::ContextFetcher::new(repo_path),
            pattern_repositories,
            review_rules,
            feedback,
            feedback_context,
            plugin_manager,
            adapter,
            verification_adapter,
            embedding_adapter,
            base_prompt_config,
            convention_store_path,
            is_local,
        })
    }

    pub(super) fn repo_path_str(&self) -> String {
        self.repo_path.to_string_lossy().to_string()
    }
}

pub(super) fn should_optimize_for_local(config: &config::Config) -> bool {
    if config.context_window.is_some() {
        return true;
    }
    if config.model.starts_with("ollama:") {
        return true;
    }
    if config.adapter.as_deref() == Some("ollama") {
        return true;
    }
    config.is_local_endpoint()
}
