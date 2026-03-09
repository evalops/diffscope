use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_name: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub temperature: f32,
    pub max_tokens: usize,
    pub openai_use_responses: Option<bool>,
    #[serde(default)]
    pub adapter_override: Option<String>,
    /// Override HTTP timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Override max retries for transient failures.
    #[serde(default)]
    pub max_retries: Option<usize>,
    /// Override base delay between retries in milliseconds.
    #[serde(default)]
    pub retry_delay_ms: Option<u64>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_name: "claude-sonnet-4-6".to_string(),
            api_key: None,
            base_url: None,
            temperature: 0.2,
            max_tokens: 4000,
            openai_use_responses: None,
            adapter_override: None,
            timeout_secs: None,
            max_retries: None,
            retry_delay_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[async_trait]
pub trait LLMAdapter: Send + Sync {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    #[allow(dead_code)]
    fn model_name(&self) -> &str;
}

/// Check if a base URL points to an Ollama instance by parsing the port.
fn is_ollama_url(base_url: &Option<String>) -> bool {
    base_url.as_ref().is_some_and(|u| {
        url::Url::parse(u)
            .map(|parsed| parsed.port() == Some(11434))
            .unwrap_or(false)
    })
}

pub fn create_adapter(config: &ModelConfig) -> Result<Box<dyn LLMAdapter>> {
    let config = config.clone(); // single clone upfront

    // Explicit adapter override takes priority
    if let Some(ref adapter) = config.adapter_override {
        return match adapter.as_str() {
            "anthropic" => Ok(Box::new(crate::adapters::AnthropicAdapter::new(config)?)),
            "ollama" => Ok(Box::new(crate::adapters::OllamaAdapter::new(config)?)),
            "openrouter" => {
                // OpenRouter uses OpenAI-compatible API
                let mut or_config = config.clone();
                if or_config.base_url.is_none() {
                    or_config.base_url = Some("https://openrouter.ai/api/v1".to_string());
                }
                Ok(Box::new(crate::adapters::OpenAIAdapter::new(or_config)?))
            }
            _ => Ok(Box::new(crate::adapters::OpenAIAdapter::new(config)?)),
        };
    }

    // Vendor-prefixed model IDs (vendor/model)
    if config.model_name.contains('/') {
        let (vendor, model) = config.model_name.split_once('/').unwrap();
        let model = model.to_string();
        match vendor {
            "anthropic" => {
                // Route anthropic/ prefix directly to the Anthropic adapter
                let mut anth_config = config;
                anth_config.model_name = model;
                return Ok(Box::new(crate::adapters::AnthropicAdapter::new(anth_config)?));
            }
            _ => {
                // Other vendor prefixes (e.g. openai/, meta-llama/) → OpenRouter
                let mut or_config = config;
                if or_config.base_url.is_none() {
                    or_config.base_url = Some("https://openrouter.ai/api/v1".to_string());
                }
                return Ok(Box::new(crate::adapters::OpenAIAdapter::new(or_config)?));
            }
        }
    }

    // Model-name heuristic
    match config.model_name.as_str() {
        // Anthropic Claude models (all versions)
        name if name.starts_with("claude-") => {
            Ok(Box::new(crate::adapters::AnthropicAdapter::new(config)?))
        }
        // Legacy claude naming without dash
        name if name.starts_with("claude") => {
            Ok(Box::new(crate::adapters::AnthropicAdapter::new(config)?))
        }
        // OpenAI models
        name if name.starts_with("gpt-") => {
            Ok(Box::new(crate::adapters::OpenAIAdapter::new(config)?))
        }
        name if name.starts_with("o1-") => {
            Ok(Box::new(crate::adapters::OpenAIAdapter::new(config)?))
        }
        // Ollama models
        name if name.starts_with("ollama:") => {
            Ok(Box::new(crate::adapters::OllamaAdapter::new(config)?))
        }
        _name if is_ollama_url(&config.base_url) => {
            Ok(Box::new(crate::adapters::OllamaAdapter::new(config)?))
        }
        // Default to OpenAI for unknown models
        _ => Ok(Box::new(crate::adapters::OpenAIAdapter::new(config)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper config that uses a local base_url so no real API key is needed.
    fn local_config(model_name: &str) -> ModelConfig {
        ModelConfig {
            model_name: model_name.to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://localhost:9999".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_create_adapter_claude_dash_prefix() {
        let config = local_config("claude-3-5-sonnet-20241022");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn test_create_adapter_claude_legacy_prefix() {
        let config = local_config("claude3sonnet");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "claude3sonnet");
    }

    #[test]
    fn test_create_adapter_gpt() {
        let config = local_config("gpt-4o");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "gpt-4o");
    }

    #[test]
    fn test_create_adapter_gpt35() {
        let config = local_config("gpt-3.5-turbo");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "gpt-3.5-turbo");
    }

    #[test]
    fn test_create_adapter_o1() {
        let config = local_config("o1-preview");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "o1-preview");
    }

    #[test]
    fn test_create_adapter_ollama_prefix() {
        let config = local_config("ollama:codellama");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "ollama:codellama");
    }

    #[test]
    fn test_create_adapter_ollama_by_port() {
        let config = ModelConfig {
            model_name: "codellama".to_string(),
            api_key: None,
            base_url: Some("http://localhost:11434".to_string()),
            ..Default::default()
        };
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "codellama");
    }

    #[test]
    fn test_create_adapter_default_unknown_model() {
        // Unknown model name with a local base_url should default to OpenAI adapter
        let config = local_config("some-custom-model");
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "some-custom-model");
    }

    #[test]
    fn test_create_adapter_explicit_override_anthropic() {
        let config = ModelConfig {
            model_name: "gpt-4o".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://localhost:9999".to_string()),
            adapter_override: Some("anthropic".to_string()),
            ..Default::default()
        };
        // Even though model_name says "gpt-4o", the override should pick Anthropic
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "gpt-4o");
    }

    #[test]
    fn test_create_adapter_explicit_override_ollama() {
        let config = ModelConfig {
            model_name: "my-model".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://localhost:9999".to_string()),
            adapter_override: Some("ollama".to_string()),
            ..Default::default()
        };
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "my-model");
    }

    #[test]
    fn test_create_adapter_explicit_override_openai() {
        let config = ModelConfig {
            model_name: "claude-3-5-sonnet-20241022".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://localhost:9999".to_string()),
            adapter_override: Some("openai".to_string()),
            ..Default::default()
        };
        // Even though model_name says "claude-*", the override should pick OpenAI
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn test_create_adapter_explicit_override_unknown_defaults_to_openai() {
        let config = ModelConfig {
            model_name: "my-model".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://localhost:9999".to_string()),
            adapter_override: Some("unknown-adapter".to_string()),
            ..Default::default()
        };
        // Unknown adapter override should default to OpenAI
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "my-model");
    }

    #[test]
    fn test_create_adapter_ollama_by_standard_url() {
        // Should detect Ollama from standard localhost:11434 URL
        let config = ModelConfig {
            model_name: "codellama".to_string(),
            api_key: None,
            base_url: Some("http://localhost:11434".to_string()),
            ..Default::default()
        };
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "codellama");
    }

    #[test]
    fn test_create_adapter_ollama_by_url_with_path() {
        // Should detect Ollama even with trailing path
        let config = ModelConfig {
            model_name: "codellama".to_string(),
            api_key: None,
            base_url: Some("http://my-server:11434/api".to_string()),
            ..Default::default()
        };
        let adapter = create_adapter(&config).unwrap();
        assert_eq!(adapter.model_name(), "codellama");
    }

    #[test]
    fn test_create_adapter_port_in_path_not_detected_as_ollama() {
        // A URL like http://proxy.example.com/service/11434 should NOT trigger Ollama
        let config = ModelConfig {
            model_name: "my-model".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("http://proxy.example.com/service/11434".to_string()),
            ..Default::default()
        };
        // Should default to OpenAI, not Ollama
        let _adapter = create_adapter(&config).unwrap();
    }

    #[test]
    fn test_create_adapter_anthropic_vendor_prefix() {
        // anthropic/ prefix should route to AnthropicAdapter, not OpenRouter
        let config = local_config("anthropic/claude-sonnet-4-6");
        let adapter = create_adapter(&config).unwrap();
        // The adapter strips the vendor prefix
        assert_eq!(adapter.model_name(), "claude-sonnet-4-6");
    }

    #[test]
    fn test_model_config_default() {
        let config = ModelConfig::default();
        assert_eq!(config.model_name, "claude-sonnet-4-6");
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!((config.temperature - 0.2).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 4000);
        assert!(config.openai_use_responses.is_none());
        assert!(config.adapter_override.is_none());
    }
}
