use anyhow::Result;
use std::sync::Arc;
use tracing::{info, warn};

use crate::adapters;
use crate::adapters::llm::ModelConfig;
use crate::config;

pub(super) struct AdapterServices {
    pub adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub verification_adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub embedding_adapter: Option<Arc<dyn adapters::llm::LLMAdapter>>,
    pub is_local: bool,
}

pub(super) fn build_adapter_services(config: &config::Config) -> Result<AdapterServices> {
    let model_config = config.to_model_config();
    let adapter: Arc<dyn adapters::llm::LLMAdapter> =
        Arc::from(adapters::llm::create_adapter(&model_config)?);
    info!("Review adapter: {}", adapter.model_name());

    Ok(AdapterServices {
        verification_adapter: build_verification_adapter(config, &model_config, &adapter)?,
        embedding_adapter: build_embedding_adapter(config, &model_config, &adapter),
        is_local: should_optimize_for_local(config),
        adapter,
    })
}

fn build_verification_adapter(
    config: &config::Config,
    model_config: &ModelConfig,
    adapter: &Arc<dyn adapters::llm::LLMAdapter>,
) -> Result<Arc<dyn adapters::llm::LLMAdapter>> {
    let verification_config = config.to_model_config_for_role(config.verification_model_role);
    if verification_config.model_name != model_config.model_name {
        info!(
            "Using '{}' model '{}' for verification pass",
            format!("{:?}", config.verification_model_role).to_lowercase(),
            verification_config.model_name
        );
        Ok(Arc::from(adapters::llm::create_adapter(
            &verification_config,
        )?))
    } else {
        Ok(adapter.clone())
    }
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
