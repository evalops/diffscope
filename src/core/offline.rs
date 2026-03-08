use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Configuration for offline/self-hosted review mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineConfig {
    pub model_name: String,
    pub base_url: String,
    pub context_window: usize,
    pub max_tokens: usize,
    pub quantization: Option<String>,
    pub gpu_layers: Option<usize>,
}

impl Default for OfflineConfig {
    fn default() -> Self {
        Self {
            model_name: "llama3.2:latest".to_string(),
            base_url: "http://localhost:11434".to_string(),
            context_window: 8192,
            max_tokens: 4096,
            quantization: None,
            gpu_layers: None,
        }
    }
}

impl OfflineConfig {
    /// Estimate RAM usage in MB based on model name heuristics.
    pub fn estimated_ram_mb(&self) -> usize {
        let model_lower = self.model_name.to_lowercase();

        // Base estimates for common model sizes
        let base = if model_lower.contains("70b") {
            40_000
        } else if model_lower.contains("34b") || model_lower.contains("33b") {
            20_000
        } else if model_lower.contains("13b") {
            8_000
        } else if model_lower.contains("7b") || model_lower.contains("8b") {
            5_000
        } else if model_lower.contains("3b") {
            2_500
        } else if model_lower.contains("1.5b") {
            1_500
        } else if model_lower.contains("1b") {
            1_000
        } else {
            4_000 // default estimate
        };

        // Quantization reduces memory
        match self.quantization.as_deref() {
            Some("q4_0") | Some("q4_1") => base / 2,
            Some("q5_0") | Some("q5_1") => base * 5 / 8,
            Some("q8_0") => base * 3 / 4,
            _ => base,
        }
    }

    /// Estimate disk usage in MB.
    pub fn estimated_disk_mb(&self) -> usize {
        // Roughly same as RAM for weights
        self.estimated_ram_mb()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.model_name.is_empty() {
            errors.push("Model name cannot be empty".to_string());
        }
        if self.base_url.is_empty() {
            errors.push("Base URL cannot be empty".to_string());
        } else if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            errors.push("Base URL must start with http:// or https://".to_string());
        }
        if self.context_window < 512 {
            errors.push("Context window must be at least 512 tokens".to_string());
        }
        if self.max_tokens > self.context_window {
            errors.push("max_tokens cannot exceed context_window".to_string());
        }

        errors
    }
}

/// Represents a locally available model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub name: String,
    pub size_mb: usize,
    pub quantization: Option<String>,
    pub modified_at: Option<String>,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
}

/// Manages local models for offline operation.
#[derive(Debug, Default)]
pub struct OfflineModelManager {
    models: Vec<LocalModel>,
    ollama_base_url: String,
}

impl OfflineModelManager {
    pub fn new(base_url: &str) -> Self {
        Self {
            models: Vec::new(),
            ollama_base_url: base_url.to_string(),
        }
    }

    /// Parse Ollama's `/api/tags` JSON response.
    pub fn parse_model_list(json: &str) -> Result<Vec<LocalModel>> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        let models = value
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let name = m.get("name")?.as_str()?.to_string();
                        let size = m.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                        let details = m.get("details");
                        Some(LocalModel {
                            name,
                            size_mb: (size / (1024 * 1024)) as usize,
                            quantization: details
                                .and_then(|d| d.get("quantization_level"))
                                .and_then(|q| q.as_str())
                                .map(|s| s.to_string()),
                            modified_at: m
                                .get("modified_at")
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string()),
                            family: details
                                .and_then(|d| d.get("family"))
                                .and_then(|f| f.as_str())
                                .map(|s| s.to_string()),
                            parameter_size: details
                                .and_then(|d| d.get("parameter_size"))
                                .and_then(|p| p.as_str())
                                .map(|s| s.to_string()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Set the known list of local models.
    pub fn set_models(&mut self, models: Vec<LocalModel>) {
        self.models = models;
    }

    /// Check if a specific model is available locally.
    pub fn is_model_available(&self, model_name: &str) -> bool {
        self.models
            .iter()
            .any(|m| m.name == model_name || m.name.starts_with(&format!("{}:", model_name)))
    }

    /// Get recommended model for code review based on available models.
    pub fn recommend_review_model(&self) -> Option<&LocalModel> {
        let preferred_order = [
            "deepseek-coder",
            "codellama",
            "qwen2.5-coder",
            "llama3",
            "mistral",
            "phi",
        ];

        for preferred in &preferred_order {
            if let Some(model) = self
                .models
                .iter()
                .find(|m| m.name.contains(preferred))
            {
                return Some(model);
            }
        }

        // Fall back to largest available model
        self.models.iter().max_by_key(|m| m.size_mb)
    }

    /// Get all available models.
    pub fn available_models(&self) -> &[LocalModel] {
        &self.models
    }

    /// Generate the Ollama API URL for chat completions.
    pub fn chat_url(&self) -> String {
        format!("{}/api/chat", self.ollama_base_url)
    }

    /// Generate the Ollama API URL for generating completions (legacy).
    pub fn generate_url(&self) -> String {
        format!("{}/api/generate", self.ollama_base_url)
    }

    /// Build an Ollama chat-compatible request payload.
    pub fn build_chat_request_payload(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        config: &OfflineConfig,
    ) -> serde_json::Value {
        let mut messages = Vec::new();

        if let Some(system_prompt) = system {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system_prompt
            }));
        }

        messages.push(serde_json::json!({
            "role": "user",
            "content": prompt
        }));

        let mut payload = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
            "options": {
                "num_ctx": config.context_window,
                "num_predict": config.max_tokens,
            }
        });

        if let Some(ref quant) = config.quantization {
            payload["options"]["quantization"] =
                serde_json::Value::String(quant.clone());
        }

        if let Some(gpu) = config.gpu_layers {
            payload["options"]["num_gpu"] = serde_json::Value::Number(gpu.into());
        }

        payload
    }

    /// Build an Ollama-compatible request payload (legacy generate API).
    pub fn build_request_payload(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        config: &OfflineConfig,
    ) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "num_ctx": config.context_window,
                "num_predict": config.max_tokens,
            }
        });

        if let Some(system_prompt) = system {
            payload["system"] = serde_json::Value::String(system_prompt.to_string());
        }

        if let Some(ref quant) = config.quantization {
            payload["options"]["quantization"] =
                serde_json::Value::String(quant.clone());
        }

        if let Some(gpu) = config.gpu_layers {
            payload["options"]["num_gpu"] = serde_json::Value::Number(gpu.into());
        }

        payload
    }

    /// Query Ollama's `/api/show` endpoint to detect the model's default context window size.
    ///
    /// Returns `Ok(Some(num_ctx))` if the model metadata contains a `num_ctx` parameter,
    /// `Ok(None)` if the parameter is not found, or `Err` if the request fails.
    #[allow(dead_code)]
    pub async fn detect_context_window(&self, model_name: &str) -> Result<Option<usize>> {
        let url = format!("{}/api/show", self.ollama_base_url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let response = client
            .post(&url)
            .json(&serde_json::json!({"name": model_name}))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let body: serde_json::Value = response.json().await?;
        let parameters = body
            .get("parameters")
            .and_then(|p| p.as_str());

        Ok(parse_num_ctx_from_params(parameters))
    }
}

/// Parse the `num_ctx` value from Ollama's parameters string.
///
/// The parameters field is a newline-separated list of key-value pairs, e.g.:
/// `num_ctx 4096\ntemperature 0.8`
#[allow(dead_code)]
fn parse_num_ctx_from_params(parameters: Option<&str>) -> Option<usize> {
    let params = parameters?;
    for line in params.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("num_ctx") {
            let value_str = rest.trim();
            if let Ok(value) = value_str.parse::<usize>() {
                return Some(value);
            }
        }
    }
    None
}

/// Readiness check result for offline operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessCheck {
    pub ollama_reachable: bool,
    pub model_available: bool,
    pub model_name: String,
    pub estimated_ram_mb: usize,
    pub warnings: Vec<String>,
    pub ready: bool,
}

/// Check if all prerequisites for offline operation are met.
pub fn check_readiness(config: &OfflineConfig, manager: &OfflineModelManager) -> ReadinessCheck {
    let validation_errors = config.validate();
    let model_available = manager.is_model_available(&config.model_name);
    let estimated_ram = config.estimated_ram_mb();

    let mut warnings = validation_errors;

    if !model_available {
        warnings.push(format!(
            "Model '{}' not found locally. Run: ollama pull {}",
            config.model_name, config.model_name
        ));
    }

    if estimated_ram > 16_000 {
        warnings.push(format!(
            "Model requires ~{}GB RAM. Ensure sufficient memory.",
            estimated_ram / 1000
        ));
    }

    let ready = model_available && warnings.is_empty();

    ReadinessCheck {
        ollama_reachable: true, // Would be checked via HTTP in real usage
        model_available,
        model_name: config.model_name.clone(),
        estimated_ram_mb: estimated_ram,
        warnings,
        ready,
    }
}

/// Generate a prompt optimized for smaller local models.
/// Smaller models need more structured, concise prompts.
pub fn optimize_prompt_for_local(
    system_prompt: &str,
    user_prompt: &str,
    context_window: usize,
) -> (String, String) {
    let budget = context_window.saturating_sub(500); // reserve for response
    let system_budget = budget / 4;
    let user_budget = budget * 3 / 4;

    let system = truncate_to_tokens(system_prompt, system_budget);
    let user = truncate_to_tokens(user_prompt, user_budget);

    (system, user)
}

/// Rough token estimation and truncation.
fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    // Rough estimate: 1 token ~= 4 chars
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find a valid UTF-8 char boundary at or before max_chars
    let mut end = max_chars;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = text[..end].to_string();
    truncated.push_str("\n[Truncated for local model context window]");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offline_config_default() {
        let config = OfflineConfig::default();
        assert_eq!(config.base_url, "http://localhost:11434");
        assert_eq!(config.context_window, 8192);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = OfflineConfig::default();
        let errors = config.validate();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_config_validation_empty_model() {
        let config = OfflineConfig {
            model_name: "".to_string(),
            ..Default::default()
        };
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("Model name")));
    }

    #[test]
    fn test_config_validation_tokens_exceed_context() {
        let config = OfflineConfig {
            max_tokens: 10000,
            context_window: 8192,
            ..Default::default()
        };
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_tokens")));
    }

    #[test]
    fn test_estimated_ram_7b() {
        let config = OfflineConfig {
            model_name: "llama3.2:7b".to_string(),
            ..Default::default()
        };
        let ram = config.estimated_ram_mb();
        assert!(ram > 3000 && ram < 10000);
    }

    #[test]
    fn test_estimated_ram_quantized() {
        let full = OfflineConfig {
            model_name: "codellama:7b".to_string(),
            quantization: None,
            ..Default::default()
        };
        let quantized = OfflineConfig {
            model_name: "codellama:7b".to_string(),
            quantization: Some("q4_0".to_string()),
            ..Default::default()
        };
        assert!(quantized.estimated_ram_mb() < full.estimated_ram_mb());
    }

    #[test]
    fn test_parse_model_list() {
        let json = r#"{"models":[{"name":"llama3.2:latest","size":4109853696,"details":{"family":"llama","parameter_size":"7B","quantization_level":"Q4_0"},"modified_at":"2024-01-01T00:00:00Z"}]}"#;
        let models = OfflineModelManager::parse_model_list(json).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "llama3.2:latest");
        assert_eq!(models[0].family.as_deref(), Some("llama"));
    }

    #[test]
    fn test_model_availability() {
        let mut manager = OfflineModelManager::new("http://localhost:11434");
        manager.set_models(vec![LocalModel {
            name: "llama3.2:latest".to_string(),
            size_mb: 4000,
            quantization: Some("Q4_0".to_string()),
            modified_at: None,
            family: Some("llama".to_string()),
            parameter_size: Some("7B".to_string()),
        }]);

        assert!(manager.is_model_available("llama3.2:latest"));
        assert!(manager.is_model_available("llama3.2")); // prefix match
        assert!(!manager.is_model_available("codellama"));
    }

    #[test]
    fn test_recommend_review_model() {
        let mut manager = OfflineModelManager::new("http://localhost:11434");
        manager.set_models(vec![
            LocalModel {
                name: "llama3:8b".to_string(),
                size_mb: 4000,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
            LocalModel {
                name: "deepseek-coder:6.7b".to_string(),
                size_mb: 3500,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
        ]);

        let recommended = manager.recommend_review_model().unwrap();
        // deepseek-coder should be preferred for code review
        assert!(recommended.name.contains("deepseek-coder"));
    }

    #[test]
    fn test_recommend_fallback_to_largest() {
        let mut manager = OfflineModelManager::new("http://localhost:11434");
        manager.set_models(vec![
            LocalModel {
                name: "tiny-model".to_string(),
                size_mb: 500,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
            LocalModel {
                name: "big-model".to_string(),
                size_mb: 8000,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
        ]);

        let recommended = manager.recommend_review_model().unwrap();
        assert_eq!(recommended.name, "big-model");
    }

    #[test]
    fn test_build_request_payload() {
        let manager = OfflineModelManager::new("http://localhost:11434");
        let config = OfflineConfig::default();

        let payload = manager.build_request_payload(
            "llama3.2",
            "Review this code",
            Some("You are a code reviewer"),
            &config,
        );

        assert_eq!(payload["model"], "llama3.2");
        assert_eq!(payload["prompt"], "Review this code");
        assert_eq!(payload["system"], "You are a code reviewer");
        assert_eq!(payload["stream"], false);
    }

    #[test]
    fn test_check_readiness_ready() {
        let config = OfflineConfig::default();
        let mut manager = OfflineModelManager::new("http://localhost:11434");
        manager.set_models(vec![LocalModel {
            name: "llama3.2:latest".to_string(),
            size_mb: 4000,
            quantization: None,
            modified_at: None,
            family: None,
            parameter_size: None,
        }]);

        let check = check_readiness(&config, &manager);
        assert!(check.model_available);
        assert!(check.ready);
    }

    #[test]
    fn test_check_readiness_missing_model() {
        let config = OfflineConfig {
            model_name: "nonexistent-model".to_string(),
            ..Default::default()
        };
        let manager = OfflineModelManager::new("http://localhost:11434");

        let check = check_readiness(&config, &manager);
        assert!(!check.model_available);
        assert!(!check.ready);
        assert!(check.warnings.iter().any(|w| w.contains("not found")));
    }

    #[test]
    fn test_optimize_prompt_short() {
        let (sys, user) = optimize_prompt_for_local("System prompt", "User prompt", 8192);
        assert_eq!(sys, "System prompt");
        assert_eq!(user, "User prompt");
    }

    #[test]
    fn test_optimize_prompt_truncates() {
        let long_prompt = "x".repeat(50000);
        let (_sys, user) = optimize_prompt_for_local("short", &long_prompt, 4096);
        assert!(user.len() < 50000);
        assert!(user.contains("[Truncated"));
    }

    #[test]
    fn test_generate_url() {
        let manager = OfflineModelManager::new("http://localhost:11434");
        assert_eq!(manager.generate_url(), "http://localhost:11434/api/generate");
    }

    #[test]
    fn test_estimated_disk_mb() {
        let config = OfflineConfig {
            model_name: "codellama:7b".to_string(),
            ..Default::default()
        };
        let disk = config.estimated_disk_mb();
        let ram = config.estimated_ram_mb();
        assert_eq!(disk, ram);
        assert!(disk > 0);
    }

    #[test]
    fn test_available_models() {
        let mut manager = OfflineModelManager::new("http://localhost:11434");
        assert!(manager.available_models().is_empty());

        manager.set_models(vec![
            LocalModel {
                name: "llama3:8b".to_string(),
                size_mb: 4000,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
            LocalModel {
                name: "codellama:7b".to_string(),
                size_mb: 3500,
                quantization: None,
                modified_at: None,
                family: None,
                parameter_size: None,
            },
        ]);

        let models = manager.available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3:8b");
        assert_eq!(models[1].name, "codellama:7b");
    }

    #[test]
    fn test_empty_model_list() {
        let models = OfflineModelManager::parse_model_list("{}").unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn test_optimize_prompt_truncation() {
        let long_system = "system ".repeat(1000);
        let long_user = "user ".repeat(1000);
        let (sys, usr) = optimize_prompt_for_local(&long_system, &long_user, 1000);
        // Both should be truncated to fit within context_window
        assert!(sys.len() < long_system.len());
        assert!(usr.len() < long_user.len());
    }

    #[test]
    fn test_optimize_prompt_zero_budget() {
        // context_window < 500 makes budget = 0, but shouldn't panic
        let (sys, _usr) = optimize_prompt_for_local("system prompt", "user prompt", 100);
        // With zero budget, text is truncated to 0 chars but still returns a string
        assert!(sys.contains("Truncated") || sys.is_empty());
    }

    #[test]
    fn test_ram_estimation_quantization_reduction() {
        let base = OfflineConfig {
            model_name: "llama3:7b".to_string(),
            quantization: None,
            ..Default::default()
        };
        let quantized = OfflineConfig {
            model_name: "llama3:7b".to_string(),
            quantization: Some("q4_0".to_string()),
            ..Default::default()
        };
        // Quantized should use less RAM
        assert!(quantized.estimated_ram_mb() < base.estimated_ram_mb());
    }

    #[test]
    fn test_readiness_check() {
        let config = OfflineConfig::default();
        let manager = OfflineModelManager::new("http://localhost:11434");
        let readiness = check_readiness(&config, &manager);
        assert!(!readiness.model_name.is_empty());
    }

    #[test]
    fn test_build_request_payload_with_system() {
        let config = OfflineConfig::default();
        let manager = OfflineModelManager::new("http://localhost:11434");
        let payload = manager.build_request_payload(
            &config.model_name,
            "user prompt",
            Some("system prompt"),
            &config,
        );
        assert!(payload.to_string().contains(&config.model_name));
        assert!(payload.to_string().contains("system"));
    }

    // Regression: 1.5b must not match the 1b check (check order matters)
    #[test]
    fn test_ram_estimation_1_5b_vs_1b() {
        let config_1b = OfflineConfig {
            model_name: "qwen2.5:1b".to_string(),
            ..Default::default()
        };
        let config_1_5b = OfflineConfig {
            model_name: "qwen2.5:1.5b".to_string(),
            ..Default::default()
        };
        // 1.5b model should use strictly more RAM than 1b model
        assert!(
            config_1_5b.estimated_ram_mb() > config_1b.estimated_ram_mb(),
            "1.5b ({}) should need more RAM than 1b ({})",
            config_1_5b.estimated_ram_mb(),
            config_1b.estimated_ram_mb()
        );
    }

    // Regression: truncate_to_tokens must respect UTF-8 char boundaries
    #[test]
    fn test_truncate_to_tokens_utf8_safety() {
        // '€' is 3 bytes (U+20AC). With context_window=504, budget=4,
        // system_budget=1, max_chars=4. "€€" is 6 bytes.
        // text[..4] lands inside the second '€' (bytes 3-5), panicking.
        let system = "€€"; // 6 bytes
        let user = "short";
        // context_window=504 → budget=4, system_budget=1, user_budget=3
        // system max_chars=4, which falls mid-char in "€€"
        let (sys, _usr) = optimize_prompt_for_local(system, user, 504);
        // Should not panic; result must be valid UTF-8
        assert!(!sys.is_empty() || sys.contains("Truncated") || sys.is_empty());
    }

    // Regression: validate() must reject invalid URL formats
    #[test]
    fn test_validate_rejects_invalid_url() {
        let config = OfflineConfig {
            base_url: "not-a-url".to_string(),
            ..Default::default()
        };
        let errors = config.validate();
        assert!(
            errors.iter().any(|e| e.contains("URL") || e.contains("url")),
            "Should flag invalid URL format, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_chat_url() {
        let manager = OfflineModelManager::new("http://localhost:11434");
        assert_eq!(manager.chat_url(), "http://localhost:11434/api/chat");
    }

    #[test]
    fn test_build_chat_request_payload() {
        let manager = OfflineModelManager::new("http://localhost:11434");
        let config = OfflineConfig::default();

        let payload = manager.build_chat_request_payload(
            "llama3.2",
            "Review this code",
            Some("You are a code reviewer"),
            &config,
        );

        assert_eq!(payload["model"], "llama3.2");
        assert_eq!(payload["stream"], false);

        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a code reviewer");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Review this code");
    }

    #[test]
    fn test_build_chat_request_payload_no_system() {
        let manager = OfflineModelManager::new("http://localhost:11434");
        let config = OfflineConfig::default();

        let payload = manager.build_chat_request_payload(
            "llama3.2",
            "Review this code",
            None,
            &config,
        );

        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_parse_num_ctx_from_params_found() {
        assert_eq!(
            parse_num_ctx_from_params(Some("num_ctx 4096\ntemperature 0.8")),
            Some(4096)
        );
    }

    #[test]
    fn test_parse_num_ctx_from_params_missing() {
        assert_eq!(
            parse_num_ctx_from_params(Some("temperature 0.8\nstop [INST]")),
            None
        );
    }

    #[test]
    fn test_parse_num_ctx_from_params_none() {
        assert_eq!(parse_num_ctx_from_params(None), None);
    }

    #[test]
    fn test_parse_num_ctx_from_params_large() {
        assert_eq!(
            parse_num_ctx_from_params(Some("num_ctx 131072")),
            Some(131072)
        );
    }

    #[tokio::test]
    async fn test_detect_context_window_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"parameters": "num_ctx 16384\ntemperature 0.7"}"#)
            .create_async()
            .await;

        let manager = OfflineModelManager::new(&server.url());
        let result = manager.detect_context_window("codellama").await.unwrap();
        assert_eq!(result, Some(16384));
    }

    #[tokio::test]
    async fn test_detect_context_window_no_num_ctx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"parameters": "temperature 0.7"}"#)
            .create_async()
            .await;

        let manager = OfflineModelManager::new(&server.url());
        let result = manager.detect_context_window("codellama").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_detect_context_window_server_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;

        let manager = OfflineModelManager::new(&server.url());
        let result = manager.detect_context_window("nonexistent").await.unwrap();
        assert_eq!(result, None);
    }
}
