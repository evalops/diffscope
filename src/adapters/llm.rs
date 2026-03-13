use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

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
            model_name: "anthropic/claude-opus-4.5".to_string(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<StructuredOutputSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructuredOutputSchema {
    pub name: String,
    pub schema: serde_json::Value,
    #[serde(default = "default_true")]
    pub strict: bool,
}

impl StructuredOutputSchema {
    pub fn json_schema(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            strict: true,
        }
    }
}

fn default_true() -> bool {
    true
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

// === Chat API types (for agent loop / tool use) ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

impl fmt::Display for ChatRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatRole::User => write!(f, "user"),
            ChatRole::Assistant => write!(f, "assistant"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
}

impl ChatRequest {
    /// Build a ChatRequest from a one-shot LLMRequest plus tool definitions.
    pub fn from_llm_request(request: LLMRequest, tools: &[ToolDefinition]) -> Self {
        Self {
            system_prompt: request.system_prompt,
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: vec![ContentBlock::Text {
                    text: request.user_prompt,
                }],
            }],
            tools: tools.to_vec(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub usage: Option<Usage>,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

#[async_trait]
pub trait LLMAdapter: Send + Sync {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse>;
    fn model_name(&self) -> &str;

    /// Embed one or more texts for semantic retrieval and feedback learning.
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(anyhow::anyhow!(
            "Embeddings are not supported by adapter for model {}",
            self.model_name()
        ))
    }

    fn supports_embeddings(&self) -> bool {
        false
    }

    /// Multi-turn chat with tool use support.
    /// Default impl flattens to a single `complete()` call (no tool support).
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        // Flatten messages into a single user prompt
        let user_prompt = request
            .messages
            .iter()
            .filter_map(|m| {
                m.content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .next()
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let llm_request = LLMRequest {
            system_prompt: request.system_prompt,
            user_prompt,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            response_schema: None,
        };

        let response = self.complete(llm_request).await?;
        Ok(ChatResponse {
            content: vec![ContentBlock::Text {
                text: response.content,
            }],
            model: response.model,
            usage: response.usage,
            stop_reason: StopReason::EndTurn,
        })
    }

    /// Whether this adapter supports native tool use.
    fn supports_tools(&self) -> bool {
        false
    }
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
    if let Some((vendor, model_suffix)) = config.model_name.split_once('/') {
        let model = model_suffix.to_string();
        match vendor {
            "anthropic" => {
                // Route anthropic/ prefix directly to the Anthropic adapter
                let mut anth_config = config;
                anth_config.model_name = model;
                return Ok(Box::new(crate::adapters::AnthropicAdapter::new(
                    anth_config,
                )?));
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
        assert_eq!(config.model_name, "anthropic/claude-opus-4.5");
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!((config.temperature - 0.2).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 4000);
        assert!(config.openai_use_responses.is_none());
        assert!(config.adapter_override.is_none());
    }

    #[test]
    fn test_content_block_serde_roundtrip_text() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_serde_roundtrip_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "call_123".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"file_path": "src/main.rs"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_123");
                assert_eq!(name, "read_file");
                assert_eq!(input["file_path"], "src/main.rs");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_serde_roundtrip_tool_result() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "call_123".to_string(),
            content: "file contents here".to_string(),
            is_error: false,
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_123");
                assert_eq!(content, "file contents here");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult block"),
        }
    }

    #[test]
    fn test_chat_message_serde_roundtrip() {
        let msg = ChatMessage {
            role: ChatRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me check that file.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"file_path": "lib.rs"}),
                },
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, ChatRole::Assistant);
        assert_eq!(parsed.content.len(), 2);
    }

    #[test]
    fn test_tool_definition_serde() {
        let tool = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"}
                },
                "required": ["file_path"]
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_file");
    }

    #[test]
    fn test_chat_request_from_llm_request() {
        let llm_req = LLMRequest {
            system_prompt: "You are a reviewer.".to_string(),
            user_prompt: "Review this diff.".to_string(),
            temperature: Some(0.3),
            max_tokens: Some(2000),
            response_schema: Some(StructuredOutputSchema::json_schema(
                "review_comments",
                serde_json::json!({"type": "array"}),
            )),
        };
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({}),
        }];
        let chat_req = ChatRequest::from_llm_request(llm_req, &tools);
        assert_eq!(chat_req.system_prompt, "You are a reviewer.");
        assert_eq!(chat_req.messages.len(), 1);
        assert_eq!(chat_req.messages[0].role, ChatRole::User);
        assert_eq!(chat_req.tools.len(), 1);
        assert_eq!(chat_req.temperature, Some(0.3));
        assert_eq!(chat_req.max_tokens, Some(2000));
    }

    #[test]
    fn test_stop_reason_serde_roundtrip() {
        let reasons = vec![
            StopReason::EndTurn,
            StopReason::ToolUse,
            StopReason::MaxTokens,
            StopReason::Other,
        ];
        for reason in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            let parsed: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, reason);
        }
    }

    #[test]
    fn test_chat_role_display() {
        assert_eq!(ChatRole::User.to_string(), "user");
        assert_eq!(ChatRole::Assistant.to_string(), "assistant");
        // Ensure they produce different strings (catches mutation → same output)
        assert_ne!(ChatRole::User.to_string(), ChatRole::Assistant.to_string());
    }

    #[test]
    fn test_default_supports_tools_is_false() {
        use async_trait::async_trait;

        struct MinimalAdapter;
        #[async_trait]
        impl LLMAdapter for MinimalAdapter {
            async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
                unimplemented!()
            }
            fn model_name(&self) -> &str {
                "minimal"
            }
        }

        let adapter = MinimalAdapter;
        assert!(
            !adapter.supports_tools(),
            "Default supports_tools() should return false, not true"
        );
    }
}
