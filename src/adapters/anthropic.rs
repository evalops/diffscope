use crate::adapters::common;
use crate::adapters::llm::{
    ChatRequest, ChatResponse, ContentBlock, LLMAdapter, LLMRequest, LLMResponse, ModelConfig,
    StopReason, Usage,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct AnthropicAdapter {
    client: Client,
    config: ModelConfig,
    api_key: String,
    base_url: String,
    retry_config: common::RetryConfig,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: usize,
    temperature: f32,
    system: String,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<Content>,
    model: String,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct Content {
    text: String,
    #[serde(rename = "type")]
    content_type: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: usize,
    output_tokens: usize,
}

// === Chat API types (for tool use) ===

#[derive(Serialize)]
struct AnthropicChatRequest {
    model: String,
    messages: Vec<AnthropicChatMessage>,
    max_tokens: usize,
    temperature: f32,
    system: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicToolDef>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AnthropicChatMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
}

#[derive(Serialize)]
struct AnthropicToolDef {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct AnthropicChatResponse {
    content: Vec<AnthropicContentBlock>,
    model: String,
    usage: AnthropicUsage,
    stop_reason: String,
}

impl AnthropicAdapter {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());

        let is_local = common::is_local_endpoint(&base_url);

        let api_key = config.api_key.clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| if is_local { Some(String::new()) } else { None })
            .context("Anthropic API key not found. Set ANTHROPIC_API_KEY environment variable or provide in config")?;

        let default_timeout = if is_local { 300 } else { 60 };
        let timeout_secs = config.timeout_secs.unwrap_or(default_timeout);
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()?;

        let retry_config = common::RetryConfig {
            max_retries: config.max_retries.unwrap_or(2),
            base_delay_ms: config.retry_delay_ms.unwrap_or(250),
        };

        Ok(Self {
            client,
            config,
            api_key,
            base_url,
            retry_config,
        })
    }
}

#[async_trait]
impl LLMAdapter for AnthropicAdapter {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let messages = vec![Message {
            role: "user".to_string(),
            content: request.user_prompt,
        }];

        let anthropic_request = AnthropicRequest {
            model: self.config.model_name.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(self.config.max_tokens),
            temperature: request.temperature.unwrap_or(self.config.temperature),
            system: request.system_prompt,
        };

        let url = format!("{}/messages", self.base_url);
        let response = common::send_with_retry_config("Anthropic", &self.retry_config, &mut || {
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", "messages-2023-12-15")
                .header("Content-Type", "application/json")
                .json(&anthropic_request)
        })
        .await
        .context("Failed to send request to Anthropic")?;

        let anthropic_response: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        let content = anthropic_response
            .content
            .first()
            .map(|c| {
                if c.content_type == "text" {
                    Ok(c.text.clone())
                } else {
                    Err(anyhow::anyhow!(
                        "Unsupported content type: {}",
                        c.content_type
                    ))
                }
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Anthropic returned empty content array — no content generated")
            })??;

        Ok(LLMResponse {
            content,
            model: anthropic_response.model,
            usage: Some(Usage {
                prompt_tokens: anthropic_response.usage.input_tokens,
                completion_tokens: anthropic_response.usage.output_tokens,
                total_tokens: anthropic_response.usage.input_tokens
                    + anthropic_response.usage.output_tokens,
            }),
        })
    }

    fn model_name(&self) -> &str {
        &self.config.model_name
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let messages: Vec<AnthropicChatMessage> = request
            .messages
            .iter()
            .map(|m| AnthropicChatMessage {
                role: m.role.to_string(),
                content: m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { text } => {
                            AnthropicContentBlock::Text { text: text.clone() }
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            AnthropicContentBlock::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            }
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => AnthropicContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: *is_error,
                        },
                    })
                    .collect(),
            })
            .collect();

        let tools: Option<Vec<AnthropicToolDef>> = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| AnthropicToolDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect(),
            )
        };

        let anthropic_request = AnthropicChatRequest {
            model: self.config.model_name.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(self.config.max_tokens),
            temperature: request.temperature.unwrap_or(self.config.temperature),
            system: request.system_prompt,
            tools,
        };

        let url = format!("{}/messages", self.base_url);
        let response = common::send_with_retry_config("Anthropic", &self.retry_config, &mut || {
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&anthropic_request)
        })
        .await
        .context("Failed to send chat request to Anthropic")?;

        let anthropic_response: AnthropicChatResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic chat response")?;

        let content: Vec<ContentBlock> = anthropic_response
            .content
            .into_iter()
            .map(|b| match b {
                AnthropicContentBlock::Text { text } => ContentBlock::Text { text },
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    ContentBlock::ToolUse { id, name, input }
                }
                AnthropicContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                },
            })
            .collect();

        let stop_reason = match anthropic_response.stop_reason.as_str() {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            _ => StopReason::Other,
        };

        Ok(ChatResponse {
            content,
            model: anthropic_response.model,
            usage: Some(Usage {
                prompt_tokens: anthropic_response.usage.input_tokens,
                completion_tokens: anthropic_response.usage.output_tokens,
                total_tokens: anthropic_response.usage.input_tokens
                    + anthropic_response.usage.output_tokens,
            }),
            stop_reason,
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::llm::{
        ChatMessage, ChatRequest, ChatRole, ContentBlock as CB, LLMAdapter, LLMRequest,
        ModelConfig, StopReason, ToolDefinition,
    };

    fn test_config(base_url: &str) -> ModelConfig {
        ModelConfig {
            model_name: "claude-3-5-sonnet-20241022".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some(base_url.to_string()),
            temperature: 0.2,
            max_tokens: 100,
            openai_use_responses: None,
            adapter_override: None,
            ..Default::default()
        }
    }

    fn test_request() -> LLMRequest {
        LLMRequest {
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            temperature: None,
            max_tokens: None,
            response_schema: None,
        }
    }

    #[tokio::test]
    async fn test_successful_completion() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "text", "text": "test response"}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 10, "output_tokens": 5}
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "test response");
        assert_eq!(response.model, "claude-3-5-sonnet-20241022");
        let usage = response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_api_error_non_retryable_401() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/messages")
            .with_status(401)
            .with_body("Unauthorized")
            .expect(1)
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("401") || err_msg.contains("Unauthorized"),
            "Error should mention 401 or Unauthorized, got: {err_msg}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retryable_error_429_all_fail() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/messages")
            .with_status(429)
            .with_body("Rate limited")
            .expect(3)
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retryable_error_500_all_fail() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/messages")
            .with_status(500)
            .with_body("Internal Server Error")
            .expect(3)
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_includes_anthropic_headers() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/messages")
            .match_header("x-api-key", "test-key")
            .match_header("anthropic-version", "2023-06-01")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "text", "text": "ok"}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_unsupported_content_type_returns_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "image", "text": "ignored"}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(
            result.is_err(),
            "Unsupported content type should return an error"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unsupported content type"),
            "Error should mention unsupported type, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_empty_content_array_returns_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 1, "output_tokens": 0}
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(
            result.is_err(),
            "Empty content array should return an error, not silently succeed"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("empty content"),
            "Error should mention empty content: {err}"
        );
    }

    #[test]
    fn test_local_endpoint_no_api_key() {
        let config = ModelConfig {
            model_name: "claude-3-5-sonnet-20241022".to_string(),
            api_key: None,
            base_url: Some("http://localhost:8080".to_string()),
            ..Default::default()
        };
        let adapter = AnthropicAdapter::new(config);
        assert!(adapter.is_ok());
    }

    #[test]
    fn test_local_endpoint_127_0_0_1_no_api_key() {
        let config = ModelConfig {
            model_name: "claude-3-5-sonnet-20241022".to_string(),
            api_key: None,
            base_url: Some("http://127.0.0.1:8080".to_string()),
            ..Default::default()
        };
        let adapter = AnthropicAdapter::new(config);
        assert!(adapter.is_ok());
    }

    #[test]
    fn testmodel_name() {
        let config = test_config("http://localhost:8080");
        let adapter = AnthropicAdapter::new(config).unwrap();
        assert_eq!(adapter.model_name(), "claude-3-5-sonnet-20241022");
    }

    #[tokio::test]
    async fn test_custom_temperature_and_max_tokens_override() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "text", "text": "ok"}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter
            .complete(LLMRequest {
                system_prompt: "s".to_string(),
                user_prompt: "u".to_string(),
                temperature: Some(0.9),
                max_tokens: Some(200),
                response_schema: None,
            })
            .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_supports_tools() {
        let config = test_config("http://localhost:8080");
        let adapter = AnthropicAdapter::new(config).unwrap();
        assert!(adapter.supports_tools());
    }

    fn make_chat_request() -> ChatRequest {
        ChatRequest {
            system_prompt: "You are a code reviewer.".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: vec![CB::Text {
                    text: "Review this.".to_string(),
                }],
            }],
            tools: vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"file_path": {"type": "string"}},
                    "required": ["file_path"]
                }),
            }],
            temperature: None,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn test_chat_end_turn() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "text", "text": "LGTM, no issues found."}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 100, "output_tokens": 20},
                    "stop_reason": "end_turn"
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.chat(make_chat_request()).await.unwrap();

        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            CB::Text { text } => assert_eq!(text, "LGTM, no issues found."),
            _ => panic!("Expected text block"),
        }
        assert_eq!(result.usage.unwrap().total_tokens, 120);
    }

    #[tokio::test]
    async fn test_chat_tool_use_response() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [
                        {"type": "text", "text": "Let me check that file."},
                        {"type": "tool_use", "id": "toolu_01", "name": "read_file", "input": {"file_path": "src/main.rs"}}
                    ],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 100, "output_tokens": 30},
                    "stop_reason": "tool_use"
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.chat(make_chat_request()).await.unwrap();

        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert_eq!(result.content.len(), 2);
        match &result.content[1] {
            CB::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read_file");
                assert_eq!(input["file_path"], "src/main.rs");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[tokio::test]
    async fn test_chat_max_tokens_stop_reason() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/messages")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "content": [{"type": "text", "text": "Partial response..."}],
                    "model": "claude-3-5-sonnet-20241022",
                    "usage": {"input_tokens": 100, "output_tokens": 100},
                    "stop_reason": "max_tokens"
                }"#,
            )
            .create_async()
            .await;

        let adapter = AnthropicAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.chat(make_chat_request()).await.unwrap();

        assert_eq!(result.stop_reason, StopReason::MaxTokens);
    }
}
