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
    retry_config: common::RetryConfig,
}

// -- Chat API types (primary) --

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: usize,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessage,
    model: String,
    done: bool,
    prompt_eval_count: Option<usize>,
    eval_count: Option<usize>,
}

// -- Legacy generate API types (kept for reference / future use) --

#[allow(dead_code)]
#[derive(Serialize)]
struct OllamaGenerateRequest {
    model: String,
    prompt: String,
    system: String,
    temperature: f32,
    num_predict: usize,
    stream: bool,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct OllamaGenerateResponse {
    response: String,
    model: String,
    done: bool,
    _context: Option<Vec<i32>>,
    _total_duration: Option<u64>,
    prompt_eval_count: Option<usize>,
    eval_count: Option<usize>,
}

// -- /api/show types for context window detection --

#[allow(dead_code)]
#[derive(Serialize)]
struct OllamaShowRequest {
    name: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct OllamaShowResponse {
    #[serde(default)]
    parameters: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    details: Option<OllamaShowDetails>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct OllamaShowDetails {
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

impl OllamaAdapter {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        let timeout_secs = config.timeout_secs.unwrap_or(300);
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
            base_url,
            retry_config,
        })
    }

    /// Strip the `ollama:` prefix from the model name if present.
    fn model_name_bare(&self) -> &str {
        self.config
            .model_name
            .strip_prefix("ollama:")
            .unwrap_or(&self.config.model_name)
    }

    /// Query Ollama's `/api/show` endpoint to detect the model's default context window size.
    ///
    /// Returns `Some(num_ctx)` if the model metadata contains a `num_ctx` parameter,
    /// or `None` if the endpoint is unreachable or the parameter is not found.
    #[allow(dead_code)]
    pub async fn detect_context_window(&self) -> Option<usize> {
        let url = format!("{}/api/show", self.base_url);
        let show_request = OllamaShowRequest {
            name: self.model_name_bare().to_string(),
        };

        let response = self.client.post(&url).json(&show_request).send().await.ok()?;

        if !response.status().is_success() {
            return None;
        }

        let show_response: OllamaShowResponse = response.json().await.ok()?;
        parse_num_ctx(show_response.parameters.as_deref())
    }
}

/// Parse the `num_ctx` value from Ollama's parameters string.
///
/// The parameters field is a newline-separated list of key-value pairs, e.g.:
/// ```text
/// num_ctx 4096
/// temperature 0.8
/// ```
#[allow(dead_code)]
fn parse_num_ctx(parameters: Option<&str>) -> Option<usize> {
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

#[async_trait]
impl LLMAdapter for OllamaAdapter {
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse> {
        let model_name = self.model_name_bare();

        let messages = vec![
            OllamaChatMessage {
                role: "system".to_string(),
                content: request.system_prompt,
            },
            OllamaChatMessage {
                role: "user".to_string(),
                content: request.user_prompt,
            },
        ];

        let chat_request = OllamaChatRequest {
            model: model_name.to_string(),
            messages,
            stream: false,
            options: OllamaOptions {
                temperature: request.temperature.unwrap_or(self.config.temperature),
                num_predict: request.max_tokens.unwrap_or(self.config.max_tokens),
            },
        };

        let url = format!("{}/api/chat", self.base_url);
        let response = common::send_with_retry_config("Ollama", &self.retry_config, &mut || {
            self.client.post(&url).json(&chat_request)
        })
        .await
        .context("Failed to send request to Ollama")?;

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .context("Failed to parse Ollama chat response")?;

        Ok(LLMResponse {
            content: chat_response.message.content,
            model: chat_response.model,
            usage: if chat_response.done {
                Some(Usage {
                    prompt_tokens: chat_response.prompt_eval_count.unwrap_or(0),
                    completion_tokens: chat_response.eval_count.unwrap_or(0),
                    total_tokens: chat_response.prompt_eval_count.unwrap_or(0)
                        + chat_response.eval_count.unwrap_or(0),
                })
            } else {
                None
            },
        })
    }

    fn model_name(&self) -> &str {
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
            ..Default::default()
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

    /// Helper to build a chat API response body.
    fn chat_response_body(content: &str, model: &str, done: bool) -> String {
        format!(
            r#"{{
                "message": {{"role": "assistant", "content": "{}"}},
                "model": "{}",
                "done": {}
            }}"#,
            content, model, done
        )
    }

    fn chat_response_body_with_usage(
        content: &str,
        model: &str,
        prompt_eval: usize,
        eval: usize,
    ) -> String {
        format!(
            r#"{{
                "message": {{"role": "assistant", "content": "{}"}},
                "model": "{}",
                "done": true,
                "prompt_eval_count": {},
                "eval_count": {}
            }}"#,
            content, model, prompt_eval, eval
        )
    }

    // ---- Chat API tests ----

    #[tokio::test]
    async fn test_successful_completion() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body_with_usage("test response", "codellama", 10, 5))
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
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"model":"codellama"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body_with_usage("ok", "codellama", 1, 1))
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
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"model":"llama3"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body_with_usage("ok", "llama3", 1, 1))
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
            .mock("POST", "/api/chat")
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
            .mock("POST", "/api/chat")
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
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body("partial", "codellama", false))
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
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body("ok", "codellama", true))
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
    fn testmodel_name_with_prefix() {
        let config = test_config("http://localhost:11434", "ollama:codellama");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.model_name(), "ollama:codellama");
    }

    #[test]
    fn testmodel_name_without_prefix() {
        let config = test_config("http://localhost:11434", "codellama");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.model_name(), "codellama");
    }

    // ---- Chat message construction tests ----

    #[tokio::test]
    async fn test_chat_messages_contain_system_and_user() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"messages":[{"role":"system","content":"be helpful"},{"role":"user","content":"review this"}]}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body("ok", "codellama", true))
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let request = LLMRequest {
            system_prompt: "be helpful".to_string(),
            user_prompt: "review this".to_string(),
            temperature: None,
            max_tokens: None,
        };
        let result = adapter.complete(request).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_options_sent_correctly() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"options":{"temperature":0.5,"num_predict":200}}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body("ok", "codellama", true))
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let request = LLMRequest {
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            temperature: Some(0.5),
            max_tokens: Some(200),
        };
        let result = adapter.complete(request).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_chat_stream_is_false() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"stream":false}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(chat_response_body("ok", "codellama", true))
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.complete(test_request()).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    // ---- Context window detection tests ----

    #[tokio::test]
    async fn test_detect_context_window_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"name":"codellama"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "modelfile": "FROM codellama",
                    "parameters": "num_ctx 4096\ntemperature 0.8\nstop [INST]",
                    "template": "...",
                    "details": {
                        "family": "llama",
                        "parameter_size": "7B",
                        "quantization_level": "Q4_0"
                    }
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, Some(4096));
    }

    #[tokio::test]
    async fn test_detect_context_window_strips_prefix() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"name":"codellama"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "parameters": "num_ctx 8192"
                }"#,
            )
            .create_async()
            .await;

        let adapter =
            OllamaAdapter::new(test_config(&server.url(), "ollama:codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, Some(8192));
    }

    #[tokio::test]
    async fn test_detect_context_window_no_num_ctx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "parameters": "temperature 0.8\nstop [INST]",
                    "details": {
                        "family": "llama"
                    }
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_detect_context_window_no_parameters() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "details": {
                        "family": "llama"
                    }
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_detect_context_window_server_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_detect_context_window_model_not_found() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(404)
            .with_body("model not found")
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "nonexistent")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_detect_context_window_large_value() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/show")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "parameters": "num_ctx 131072\ntemperature 0.7"
                }"#,
            )
            .create_async()
            .await;

        let adapter = OllamaAdapter::new(test_config(&server.url(), "codellama")).unwrap();
        let result = adapter.detect_context_window().await;

        assert_eq!(result, Some(131072));
    }

    // ---- parse_num_ctx unit tests ----

    #[test]
    fn test_parse_num_ctx_present() {
        assert_eq!(
            parse_num_ctx(Some("num_ctx 4096\ntemperature 0.8")),
            Some(4096)
        );
    }

    #[test]
    fn test_parse_num_ctx_only_entry() {
        assert_eq!(parse_num_ctx(Some("num_ctx 2048")), Some(2048));
    }

    #[test]
    fn test_parse_num_ctx_missing() {
        assert_eq!(
            parse_num_ctx(Some("temperature 0.8\nstop [INST]")),
            None
        );
    }

    #[test]
    fn test_parse_num_ctx_none_input() {
        assert_eq!(parse_num_ctx(None), None);
    }

    #[test]
    fn test_parse_num_ctx_empty_string() {
        assert_eq!(parse_num_ctx(Some("")), None);
    }

    #[test]
    fn test_parse_num_ctx_invalid_value() {
        assert_eq!(parse_num_ctx(Some("num_ctx notanumber")), None);
    }

    #[test]
    fn test_parse_num_ctx_middle_of_params() {
        let params = "temperature 0.8\nnum_ctx 16384\nstop [/INST]";
        assert_eq!(parse_num_ctx(Some(params)), Some(16384));
    }

    #[test]
    fn test_parse_num_ctx_with_whitespace() {
        assert_eq!(parse_num_ctx(Some("  num_ctx 32768  ")), Some(32768));
    }

    // ---- model_name_bare tests ----

    #[test]
    fn testmodel_name_bare_with_prefix() {
        let config = test_config("http://localhost:11434", "ollama:deepseek-coder");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.model_name_bare(), "deepseek-coder");
    }

    #[test]
    fn testmodel_name_bare_without_prefix() {
        let config = test_config("http://localhost:11434", "deepseek-coder");
        let adapter = OllamaAdapter::new(config).unwrap();
        assert_eq!(adapter.model_name_bare(), "deepseek-coder");
    }
}
