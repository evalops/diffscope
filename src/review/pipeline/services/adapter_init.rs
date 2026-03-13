use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{info, warn};

use crate::adapters;
use crate::adapters::llm::ModelConfig;
use crate::config;

pub(super) struct AdapterServices {
    pub adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub verification_adapters: Vec<Arc<dyn adapters::llm::LLMAdapter>>,
    pub embedding_adapter: Option<Arc<dyn adapters::llm::LLMAdapter>>,
    pub is_local: bool,
}

pub(super) fn build_adapter_services(config: &config::Config) -> Result<AdapterServices> {
    let model_config = config.to_model_config();
    let adapter: Arc<dyn adapters::llm::LLMAdapter> =
        Arc::from(adapters::llm::create_adapter(&model_config)?);
    info!("Review adapter: {}", adapter.model_name());

    Ok(AdapterServices {
        verification_adapters: build_verification_adapters(config, &model_config, &adapter)?,
        embedding_adapter: build_embedding_adapter(config, &model_config, &adapter),
        is_local: should_optimize_for_local(config),
        adapter,
    })
}

fn build_verification_adapters(
    config: &config::Config,
    model_config: &ModelConfig,
    adapter: &Arc<dyn adapters::llm::LLMAdapter>,
) -> Result<Vec<Arc<dyn adapters::llm::LLMAdapter>>> {
    let mut verification_adapters = Vec::new();
    let mut seen_models = HashSet::new();
    let mut roles = vec![config.verification_model_role];
    roles.extend(config.verification_additional_model_roles.iter().copied());

    for role in roles {
        let verification_config = config.to_model_config_for_role(role);
        if !seen_models.insert(verification_config.model_name.clone()) {
            continue;
        }

        if verification_config.model_name != model_config.model_name {
            info!(
                "Using '{}' model '{}' for verification pass",
                format!("{:?}", role).to_lowercase(),
                verification_config.model_name
            );
            verification_adapters.push(Arc::from(adapters::llm::create_adapter(
                &verification_config,
            )?));
        } else {
            verification_adapters.push(adapter.clone());
        }
    }

    if verification_adapters.is_empty() {
        verification_adapters.push(adapter.clone());
    }

    Ok(verification_adapters)
}

fn build_embedding_adapter(
    config: &config::Config,
    model_config: &ModelConfig,
    adapter: &Arc<dyn adapters::llm::LLMAdapter>,
) -> Option<Arc<dyn adapters::llm::LLMAdapter>> {
    if !(config.semantic_rag || config.semantic_feedback) {
        return None;
    }

    let embedding_config = config.to_model_config_for_role(config::ModelRole::Embedding);
    if embedding_config.model_name == model_config.model_name {
        return Some(adapter.clone());
    }

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
