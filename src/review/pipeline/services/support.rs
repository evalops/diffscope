use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config;
use crate::core;
use crate::plugins;

use super::super::super::context_helpers::{resolve_pattern_repositories, PatternRepositoryMap};
use super::super::super::feedback::{
    generate_feedback_context, load_feedback_store, FeedbackStore,
};
use super::super::super::rule_helpers::load_review_rules;
use super::super::repo_support::resolve_convention_store_path;

pub(super) struct SupportServices {
    pub convention_store_path: Option<PathBuf>,
    pub pattern_repositories: PatternRepositoryMap,
    pub review_rules: Vec<core::ReviewRule>,
    pub feedback: FeedbackStore,
    pub feedback_context: String,
    pub plugin_manager: plugins::plugin::PluginManager,
    pub base_prompt_config: core::prompt::PromptConfig,
}

pub(super) async fn build_support_services(
    config: &config::Config,
    repo_path: &Path,
) -> Result<SupportServices> {
    let convention_store_path = resolve_convention_store_path(config);
    let pattern_repositories = resolve_pattern_repositories(config, repo_path);
    let review_rules = load_review_rules(config, &pattern_repositories, repo_path);

    let mut plugin_manager = plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;

    let feedback = load_feedback_store(config);
    let feedback_context = if config.enhanced_feedback {
        generate_feedback_context(&feedback)
    } else {
        String::new()
    };

    let base_prompt_config = core::prompt::PromptConfig {
        max_context_chars: config.max_context_chars,
        max_diff_chars: config.max_diff_chars,
        ..Default::default()
    };

    Ok(SupportServices {
        convention_store_path,
        pattern_repositories,
        review_rules,
        feedback,
        feedback_context,
        plugin_manager,
        base_prompt_config,
    })
}
