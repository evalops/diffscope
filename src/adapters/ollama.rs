use crate::adapters::common;
use crate::adapters::llm::{LLMAdapter, LLMRequest, LLMResponse, ModelConfig, Usage};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OllamaAdapter {
    client: Client,
    config: ModelConfig,
    base_url: String,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    system: String,
    temperature: f32,
    num_predict: usize,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
    model: String,
    done: bool,
    _context: Option<Vec<i32>>,
    _total_duration: Option<u64>,
    prompt_eval_count: Option<usize>,
    eval_count: Option<usize>,
}

impl OllamaAdapter {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        Ok(Self {
            client,
            config,
            base_url,
        })
    }

}

#[async_trait]
impl LLMAdapter for OllamaAdapter {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let model_name = self
            .config
            .model_name
            .strip_prefix("ollama:")
            .unwrap_or(&self.config.model_name);

        let ollama_request = OllamaRequest {
            model: model_name.to_string(),
            prompt: request.user_prompt,
            system: request.system_prompt,
            temperature: request.temperature.unwrap_or(self.config.temperature),
            num_predict: request.max_tokens.unwrap_or(self.config.max_tokens),
            stream: false,
        };

        let url = format!("{}/api/generate", self.base_url);
        let response = common::send_with_retry("Ollama", || {
            self.client.post(&url).json(&ollama_request)
        })
        .await
        .context("Failed to send request to Ollama")?;

        let ollama_response: OllamaResponse = response
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        Ok(LLMResponse {
            content: ollama_response.response,
            model: ollama_response.model,
            usage: if ollama_response.done {
                Some(Usage {
                    prompt_tokens: ollama_response.prompt_eval_count.unwrap_or(0),
                    completion_tokens: ollama_response.eval_count.unwrap_or(0),
                    total_tokens: ollama_response.prompt_eval_count.unwrap_or(0)
                        + ollama_response.eval_count.unwrap_or(0),
                })
            } else {
                None
            },
        })
    }

    fn _model_name(&self) -> &str {
        &self.config.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::llm::{LLMAdapter, LLMRequest, ModelConfig};

    fn test_config(base_url: &str, model_name: &str) -> ModelConfig {
        ModelConfig {
            model_name: model_name.to_string(),
            api_key: None,
            base_url: Some(base_url.to_string()),
            temperature: 0.2,
            max_tokens: 100,
            openai_use_responses: None,
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
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "response": "test response",
                    "model": "codellama",
                    "done": true,
                    "prompt_eval_count": 10,
                    "eval_count": 5
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "test response");
        assert_eq!(response.model, "codellama");
        let usage = response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_strips_ollama_prefix() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"model":"codellama"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "response": "ok",
                    "model": "codellama",
                    "done": true,
                    "prompt_eval_count": 1,
                    "eval_count": 1
                }"#,
            )
            .create_async()
            .await;

        let adapter =
            OllamaAdapter::new(test_config(&server.url(), "ollama:codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_model_without_prefix_sent_as_is() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"model":"llama3"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "response": "ok",
                    "model": "llama3",
                    "done": true,
                    "prompt_eval_count": 1,
                    "eval_count": 1
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "llama3")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_api_error_non_retryable() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .with_status(404)
            .with_body("Model not found")
            .expect(1)
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retryable_error_500_all_fail() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .with_status(500)
            .with_body("Internal Server Error")
            .expect(3)
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_done_false_returns_no_usage() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "response": "partial",
                    "model": "codellama",
                    "done": false
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "partial");
        assert!(response.usage.is_none());
    }

    #[tokio::test]
    async fn test_missing_eval_counts_default_to_zero() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "response": "ok",
                    "model": "codellama",
                    "done": true
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        let usage = response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_default_base_url() {
        let config = ModelConfig {
            model_name: "codellama".to_string(),
            base_url: None,
            ..Default::default()
        };
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_custom_base_url() {
        let config = ModelConfig {
            model_name: "codellama".to_string(),
            base_url: Some("http://192.168.1.100:11434".to_string()),
            ..Default::default()
        };
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.base_url, "http://192.168.1.100:11434");
    }

    #[test]
    fn test_model_name_with_prefix() {
        let config = test_config("http://localhost:11434", "ollama:codellama");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter._model_name(), "ollama:codellama");
    }

    #[test]
    fn test_model_name_without_prefix() {
        let config = test_config("http://localhost:11434", "codellama");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter._model_name(), "codellama");
    }
}
