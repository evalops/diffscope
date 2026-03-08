use crate::adapters::common;
use crate::adapters::llm::{LLMAdapter, LLMRequest, LLMResponse, ModelConfig, Usage};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAIAdapter {
    client: Client,
    config: ModelConfig,
    api_key: String,
    base_url: String,
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: usize,
}

#[derive(Serialize)]
struct OpenAIResponsesRequest {
    model: String,
    input: String,
    instructions: String,
    temperature: f32,
    max_output_tokens: usize,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
    usage: OpenAIUsage,
    model: String,
}

#[derive(Deserialize)]
struct OpenAIResponsesResponse {
    output: Vec<OpenAIResponseOutput>,
    model: String,
    #[serde(default)]
    usage: Option<OpenAIResponsesUsage>,
}

#[derive(Deserialize)]
struct OpenAIResponseOutput {
    #[serde(rename = "type")]
    output_type: String,
    #[serde(default)]
    content: Vec<OpenAIResponseContent>,
}

#[derive(Deserialize)]
struct OpenAIResponseContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

#[derive(Deserialize)]
struct OpenAIResponsesUsage {
    input_tokens: usize,
    output_tokens: usize,
    total_tokens: usize,
}

impl OpenAIAdapter {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let is_local = common::is_local_endpoint(&base_url);

        let api_key = config.api_key.clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| if is_local { Some(String::new()) } else { None })
            .context("OpenAI API key not found. Set OPENAI_API_KEY environment variable or provide in config")?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(if is_local { 300 } else { 60 }))
            .build()?;

        Ok(Self {
            client,
            config,
            api_key,
            base_url,
        })
    }

}

#[async_trait]
impl LLMAdapter for OpenAIAdapter {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        if should_use_responses_api(&self.config) {
            return self.complete_responses(request).await;
        }

        self.complete_chat_completions(request).await
    }

    fn _model_name(&self) -> &str {
        &self.config.model_name
    }
}

fn should_use_responses_api(config: &ModelConfig) -> bool {
    if let Some(flag) = config.openai_use_responses {
        return flag;
    }

    if let Some(base_url) = config.base_url.as_ref() {
        if !base_url.contains("openai.com") {
            return false;
        }
    }

    !config.model_name.starts_with("gpt-3.5")
}

impl OpenAIAdapter {
    async fn complete_chat_completions(&self, request: LLMRequest) -> Result<LLMResponse> {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: request.system_prompt,
            },
            Message {
                role: "user".to_string(),
                content: request.user_prompt,
            },
        ];

        let openai_request = OpenAIRequest {
            model: self.config.model_name.clone(),
            messages,
            temperature: request.temperature.unwrap_or(self.config.temperature),
            max_tokens: request.max_tokens.unwrap_or(self.config.max_tokens),
        };

        let url = format!("{}/chat/completions", self.base_url);
        let response = common::send_with_retry("OpenAI", || {
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&openai_request)
        })
        .await
        .context("Failed to send request to OpenAI")?;

        let openai_response: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        let content = openai_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(LLMResponse {
            content,
            model: openai_response.model,
            usage: Some(Usage {
                prompt_tokens: openai_response.usage.prompt_tokens,
                completion_tokens: openai_response.usage.completion_tokens,
                total_tokens: openai_response.usage.total_tokens,
            }),
        })
    }

    async fn complete_responses(&self, request: LLMRequest) -> Result<LLMResponse> {
        let openai_request = OpenAIResponsesRequest {
            model: self.config.model_name.clone(),
            input: request.user_prompt,
            instructions: request.system_prompt,
            temperature: request.temperature.unwrap_or(self.config.temperature),
            max_output_tokens: request.max_tokens.unwrap_or(self.config.max_tokens),
        };

        let url = format!("{}/responses", self.base_url);
        let response = common::send_with_retry("OpenAI", || {
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&openai_request)
        })
        .await
        .context("Failed to send request to OpenAI")?;

        let openai_response: OpenAIResponsesResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        let content = extract_response_text(&openai_response);
        let usage = openai_response.usage.map(|usage| Usage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        });

        Ok(LLMResponse {
            content,
            model: openai_response.model,
            usage,
        })
    }
}

fn extract_response_text(response: &OpenAIResponsesResponse) -> String {
    let mut combined = String::new();

    for item in &response.output {
        if item.output_type != "message" {
            continue;
        }
        for content in &item.content {
            if content.content_type == "output_text" {
                if let Some(text) = &content.text {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(text);
                }
            }
        }
    }

    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::llm::{LLMAdapter, LLMRequest, ModelConfig};

    fn test_config(base_url: &str) -> ModelConfig {
        ModelConfig {
            model_name: "gpt-4o".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some(base_url.to_string()),
            temperature: 0.2,
            max_tokens: 100,
            openai_use_responses: Some(false),
            adapter_override: None,
        }
    }

    fn test_request() -> LLMRequest {
        LLMRequest {
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            temperature: None,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn test_successful_completion() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "choices": [{"message": {"role": "assistant", "content": "test response"}}],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
                    "model": "gpt-4o"
                }"#,
            )
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "test response");
        assert_eq!(response.model, "gpt-4o");
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
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body("Unauthorized")
            .expect(1)
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("401") || err_msg.contains("Unauthorized"),
            "Error should mention 401 or Unauthorized, got: {}",
            err_msg
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_api_error_non_retryable_403() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(403)
            .with_body("Forbidden")
            .expect(1)
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retryable_error_429_all_fail() {
        // 429 should retry up to MAX_RETRIES (2), so 3 total attempts
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body("Rate limited")
            .expect(3)
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("429") || err_msg.contains("Rate limited"),
            "Error should mention rate limiting, got: {}",
            err_msg
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retryable_error_500_all_fail() {
        // Server errors should also retry (3 total attempts)
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(500)
            .with_body("Internal Server Error")
            .expect(3)
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_empty_choices_returns_empty_content() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "choices": [],
                    "usage": {"prompt_tokens": 5, "completion_tokens": 0, "total_tokens": 5},
                    "model": "gpt-4o"
                }"#,
            )
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "");
    }

    #[test]
    fn test_local_endpoint_no_api_key() {
        let config = ModelConfig {
            model_name: "local-model".to_string(),
            api_key: None,
            base_url: Some("http://localhost:8080".to_string()),
            ..Default::default()
        };
        let adapter = OpenAIAdapter::new(config);
        assert!(adapter.is_ok());
    }

    #[test]
    fn test_local_endpoint_127_0_0_1_no_api_key() {
        let config = ModelConfig {
            model_name: "local-model".to_string(),
            api_key: None,
            base_url: Some("http://127.0.0.1:8080".to_string()),
            ..Default::default()
        };
        let adapter = OpenAIAdapter::new(config);
        assert!(adapter.is_ok());
    }

    #[test]
    fn test_should_use_responses_api_explicit_true() {
        let config = ModelConfig {
            openai_use_responses: Some(true),
            ..Default::default()
        };
        assert!(should_use_responses_api(&config));
    }

    #[test]
    fn test_should_use_responses_api_explicit_false() {
        let config = ModelConfig {
            openai_use_responses: Some(false),
            ..Default::default()
        };
        assert!(!should_use_responses_api(&config));
    }

    #[test]
    fn test_should_use_responses_api_non_openai_base_url() {
        let config = ModelConfig {
            openai_use_responses: None,
            base_url: Some("http://localhost:8080".to_string()),
            ..Default::default()
        };
        assert!(!should_use_responses_api(&config));
    }

    #[test]
    fn test_should_use_responses_api_gpt35_disabled() {
        let config = ModelConfig {
            model_name: "gpt-3.5-turbo".to_string(),
            openai_use_responses: None,
            ..Default::default()
        };
        assert!(!should_use_responses_api(&config));
    }

    #[test]
    fn test_should_use_responses_api_gpt4o_default() {
        let config = ModelConfig {
            model_name: "gpt-4o".to_string(),
            openai_use_responses: None,
            base_url: Some("https://api.openai.com/v1".to_string()),
            ..Default::default()
        };
        assert!(should_use_responses_api(&config));
    }

    #[test]
    fn test_extract_response_text_single_message() {
        let response = OpenAIResponsesResponse {
            output: vec![OpenAIResponseOutput {
                output_type: "message".to_string(),
                content: vec![OpenAIResponseContent {
                    content_type: "output_text".to_string(),
                    text: Some("hello world".to_string()),
                }],
            }],
            model: "gpt-4o".to_string(),
            usage: None,
        };
        assert_eq!(extract_response_text(&response), "hello world");
    }

    #[test]
    fn test_extract_response_text_non_message_skipped() {
        let response = OpenAIResponsesResponse {
            output: vec![OpenAIResponseOutput {
                output_type: "tool_call".to_string(),
                content: vec![OpenAIResponseContent {
                    content_type: "output_text".to_string(),
                    text: Some("should be ignored".to_string()),
                }],
            }],
            model: "gpt-4o".to_string(),
            usage: None,
        };
        assert_eq!(extract_response_text(&response), "");
    }

    #[test]
    fn test_extract_response_text_empty_output() {
        let response = OpenAIResponsesResponse {
            output: vec![],
            model: "gpt-4o".to_string(),
            usage: None,
        };
        assert_eq!(extract_response_text(&response), "");
    }

    #[test]
    fn test_extract_response_text_multiple_blocks_joined() {
        let response = OpenAIResponsesResponse {
            output: vec![OpenAIResponseOutput {
                output_type: "message".to_string(),
                content: vec![
                    OpenAIResponseContent {
                        content_type: "output_text".to_string(),
                        text: Some("first".to_string()),
                    },
                    OpenAIResponseContent {
                        content_type: "output_text".to_string(),
                        text: Some("second".to_string()),
                    },
                ],
            }],
            model: "gpt-4o".to_string(),
            usage: None,
        };
        assert_eq!(extract_response_text(&response), "first\nsecond");
    }

    #[test]
    fn test_model_name() {
        let config = test_config("http://localhost:8080");
        let adapter = OpenAIAdapter::new(config).unwrap();
        assert_eq!(adapter._model_name(), "gpt-4o");
    }

    #[tokio::test]
    async fn test_request_includes_auth_header() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .match_header("Authorization", "Bearer test-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "choices": [{"message": {"role": "assistant", "content": "ok"}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                    "model": "gpt-4o"
                }"#,
            )
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_custom_temperature_and_max_tokens_override() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "choices": [{"message": {"role": "assistant", "content": "ok"}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                    "model": "gpt-4o"
                }"#,
            )
            .create_async()
            .await;

        let adapter = OpenAIAdapter::new(test_config(&server.url())).unwrap();
        let result = adapter
            .complete(LLMRequest {
                system_prompt: "s".to_string(),
                user_prompt: "u".to_string(),
                temperature: Some(0.8),
                max_tokens: Some(500),
            })
            .await;

        assert!(result.is_ok());
    }
}
