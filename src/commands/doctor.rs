use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;
use crate::core::offline::{self, OfflineConfig, OfflineModelManager};

pub async fn doctor_command(config: Config) -> Result<()> {
    println!("DiffScope Doctor");
    println!("================\n");

    // System resources
    check_system_resources();

    // Config summary
    println!("Configuration:");
    println!("  Model:    {}", config.model);
    println!(
        "  Adapter:  {}",
        config.adapter.as_deref().unwrap_or("(auto-detect)")
    );
    println!(
        "  Base URL: {}",
        config.base_url.as_deref().unwrap_or("(default)")
    );
    println!(
        "  API Key:  {}",
        if config.api_key.is_some() {
            "set"
        } else {
            "not set"
        }
    );
    if let Some(cw) = config.context_window {
        println!("  Context:  {} tokens", cw);
    }
    println!();

    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".to_string());

    // Endpoint reachability
    print!("Checking endpoint {}... ", base_url);
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // Try Ollama /api/tags first
    let ollama_url = format!("{}/api/tags", base_url);
    let openai_url = format!("{}/v1/models", base_url);

    let mut models = Vec::new();
    let endpoint_type;

    if let Ok(resp) = client.get(&ollama_url).send().await {
        if resp.status().is_success() {
            println!("reachable (Ollama)");
            endpoint_type = "ollama";
            if let Ok(body) = resp.text().await {
                models = OfflineModelManager::parse_model_list(&body).unwrap_or_default();
            }
        } else if let Ok(resp2) = client.get(&openai_url).send().await {
            if resp2.status().is_success() {
                println!("reachable (OpenAI-compatible)");
                endpoint_type = "openai-compatible";
                if let Ok(body) = resp2.text().await {
                    parse_openai_models(&body, &mut models);
                }
            } else {
                return print_unreachable(&base_url);
            }
        } else {
            return print_unreachable(&base_url);
        }
    } else if let Ok(resp) = client.get(&openai_url).send().await {
        if resp.status().is_success() {
            println!("reachable (OpenAI-compatible)");
            endpoint_type = "openai-compatible";
            if let Ok(body) = resp.text().await {
                parse_openai_models(&body, &mut models);
            }
        } else {
            return print_unreachable(&base_url);
        }
    } else {
        return print_unreachable(&base_url);
    }

    // Available models
    println!("\nEndpoint type: {}", endpoint_type);
    println!("\nAvailable models ({}):", models.len());
    if models.is_empty() {
        println!("  (none found)");
        if endpoint_type == "ollama" {
            println!("\n  Pull a model: ollama pull codellama");
        }
    } else {
        for model in &models {
            let size_info = if model.size_mb > 0 {
                format!(" ({}MB", model.size_mb)
                    + &model
                        .quantization
                        .as_ref()
                        .map(|q| format!(", {}", q))
                        .unwrap_or_default()
                    + ")"
            } else {
                String::new()
            };
            println!("  - {}{}", model.name, size_info);
        }
    }

    // Recommendation
    if !models.is_empty() {
        let mut manager = OfflineModelManager::new(&base_url);
        manager.set_models(models.clone());

        if let Some(recommended) = manager.recommend_review_model() {
            println!("\nRecommended for code review: {}", recommended.name);

            let offline_config = OfflineConfig {
                model_name: recommended.name.clone(),
                base_url: base_url.clone(),
                context_window: config.context_window.unwrap_or(8192),
                max_tokens: config.max_tokens,
                ..Default::default()
            };
            let ram = offline_config.estimated_ram_mb();
            println!("  Estimated RAM: ~{}MB", ram);

            // Context window detection (Ollama only)
            if endpoint_type == "ollama" {
                if let Some(ctx_size) =
                    detect_model_context_window(&client, &base_url, &recommended.name).await
                {
                    println!("  Context window: {} tokens (detected from model)", ctx_size);
                }
            }

            // Readiness check
            let readiness = offline::check_readiness(&offline_config, &manager);
            if readiness.ready {
                println!("\nStatus: READY");
            } else {
                println!("\nStatus: NOT READY");
                for warning in &readiness.warnings {
                    println!("  Warning: {}", warning);
                }
            }

            // Test inference
            let recommended_name = recommended.name.clone();
            print!("\nTesting model {}... ", recommended_name);
            let test_client = Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?;
            let test_start = std::time::Instant::now();
            let test_result =
                test_model_inference(&test_client, &base_url, &recommended_name, endpoint_type)
                    .await;
            let elapsed = test_start.elapsed();

            match test_result {
                Ok(response) => {
                    let tokens_per_sec =
                        estimate_tokens(&response) as f64 / elapsed.as_secs_f64();
                    println!(
                        "OK ({:.1}s, ~{:.0} tok/s)",
                        elapsed.as_secs_f64(),
                        tokens_per_sec
                    );
                    if tokens_per_sec < 2.0 {
                        println!(
                            "  Warning: Very slow inference. Consider a smaller/quantized model."
                        );
                    }
                }
                Err(e) => {
                    println!("FAILED");
                    println!("  Error: {}", e);
                    println!("  The model may still be loading. Try again in a moment.");
                }
            }

            // Usage hint
            let model_flag = if endpoint_type == "ollama" {
                format!("ollama:{}", recommended_name)
            } else {
                recommended_name
            };
            println!("\nUsage:");
            println!(
                "  git diff | diffscope review --base-url {} --model {}",
                base_url, model_flag
            );
        }
    }

    Ok(())
}

/// Send a small test prompt to verify the model can produce output.
async fn test_model_inference(
    client: &Client,
    base_url: &str,
    model_name: &str,
    endpoint_type: &str,
) -> Result<String> {
    let system_msg = "You are a code reviewer. Respond with a single JSON object.";
    let user_msg = "Review this code change:\n+fn add(a: i32, b: i32) -> i32 { a + b }\nRespond with: {\"ok\": true}";

    let messages = serde_json::json!([
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ]);

    if endpoint_type == "ollama" {
        let url = format!("{}/api/chat", base_url);
        let body = serde_json::json!({
            "model": model_name,
            "messages": messages,
            "stream": false,
            "options": {"num_predict": 50}
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }

        let text = resp.text().await?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        let content = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        Ok(content)
    } else {
        // OpenAI-compatible
        let url = format!("{}/v1/chat/completions", base_url);
        let body = serde_json::json!({
            "model": model_name,
            "messages": messages,
            "max_tokens": 50,
            "temperature": 0.1
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} - {}", status, body);
        }

        let text = resp.text().await?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        let content = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        Ok(content)
    }
}

/// Rough token count estimate (~4 chars per token).
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

/// Query Ollama's `/api/show` to detect the model's context window size.
async fn detect_model_context_window(
    client: &Client,
    base_url: &str,
    model_name: &str,
) -> Option<usize> {
    let url = format!("{}/api/show", base_url);
    let body = serde_json::json!({"name": model_name});
    let resp = client.post(&url).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    // The "parameters" field is a newline-delimited string of key-value pairs
    if let Some(params) = value.get("parameters").and_then(|p| p.as_str()) {
        for line in params.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("num_ctx") {
                if let Some(val) = trimmed.split_whitespace().nth(1) {
                    return val.parse().ok();
                }
            }
        }
    }

    // Also check model_info for context_length
    if let Some(info) = value.get("model_info") {
        // Try common key patterns
        for key in &[
            "context_length",
            "llama.context_length",
            "general.context_length",
        ] {
            if let Some(ctx) = info.get(*key).and_then(|v| v.as_u64()) {
                return Some(ctx as usize);
            }
        }
    }

    None
}

/// Print system resource information (RAM and GPU if available).
fn check_system_resources() {
    println!("System Resources:");

    if let Some(total_ram_gb) = get_total_ram_gb() {
        println!("  Total RAM: {:.1} GB", total_ram_gb);
    }

    // Check for NVIDIA GPU
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total,memory.free")
        .arg("--format=csv,noheader,nounits")
        .output()
    {
        if output.status.success() {
            let gpu_info = String::from_utf8_lossy(&output.stdout);
            for line in gpu_info.trim().lines() {
                println!("  GPU: {}", line.trim());
            }
        }
    }

    // Check for Apple Silicon GPU (macOS)
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .arg("-n")
            .arg("machdep.cpu.brand_string")
            .output()
        {
            if output.status.success() {
                let cpu = String::from_utf8_lossy(&output.stdout);
                let cpu = cpu.trim();
                if cpu.contains("Apple") {
                    println!("  Chip: {} (unified memory, GPU acceleration available)", cpu);
                }
            }
        }
    }

    println!();
}

/// Get total system RAM in GB.
fn get_total_ram_gb() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        get_total_ram_gb_macos()
    }
    #[cfg(target_os = "linux")]
    {
        get_total_ram_gb_linux()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn get_total_ram_gb_macos() -> Option<f64> {
    let output = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let bytes: u64 = text.trim().parse().ok()?;
    Some(bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg(target_os = "linux")]
fn get_total_ram_gb_linux() -> Option<f64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            // Format: "MemTotal:       16384000 kB"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                return Some(kb as f64 / (1024.0 * 1024.0));
            }
        }
    }
    None
}

fn parse_openai_models(body: &str, models: &mut Vec<offline::LocalModel>) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(data) = value.get("data").and_then(|d| d.as_array()) {
            for m in data {
                if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                    models.push(offline::LocalModel {
                        name: id.to_string(),
                        size_mb: 0,
                        quantization: None,
                        modified_at: None,
                        family: None,
                        parameter_size: None,
                    });
                }
            }
        }
    }
}

fn print_unreachable(base_url: &str) -> Result<()> {
    println!("UNREACHABLE");
    println!(
        "\nCannot reach {}. Make sure your LLM server is running.",
        base_url
    );
    println!("\nQuick start:");
    println!("  Ollama:    ollama serve");
    println!("  vLLM:      vllm serve <model>");
    println!("  LM Studio: Start the app and enable the local server");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_config_defaults() {
        let config = Config::default();
        assert!(config.adapter.is_none());
        assert!(config.context_window.is_none());
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 1); // min of 1
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcdefgh"), 2); // 8 chars = 2 tokens
        assert_eq!(estimate_tokens("a]"), 1); // 2 chars / 4 = 0 -> max(0,1) = 1
    }

    #[test]
    fn test_estimate_tokens_longer_text() {
        let text = "This is a longer response with several words in it for testing.";
        let tokens = estimate_tokens(text);
        // 63 chars / 4 = 15
        assert!(tokens > 10);
        assert!(tokens < 30);
    }

    #[test]
    fn test_parse_openai_models_valid() {
        let body = r#"{"data":[{"id":"gpt-3.5-turbo"},{"id":"codellama-7b"}]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "gpt-3.5-turbo");
        assert_eq!(models[1].name, "codellama-7b");
    }

    #[test]
    fn test_parse_openai_models_empty() {
        let body = r#"{"data":[]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_invalid_json() {
        let body = "not json";
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_missing_data() {
        let body = r#"{"models":[]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_missing_id() {
        let body = r#"{"data":[{"name":"model-1"}]}"#;
        let mut models = Vec::new();
        parse_openai_models(body, &mut models);
        assert!(models.is_empty());
    }

    #[test]
    fn test_get_total_ram_gb() {
        // This should not panic on any platform
        let ram = get_total_ram_gb();
        // On macOS/Linux this should return Some; on other platforms None
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(ram.is_some(), "Should detect RAM on macOS/Linux");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let gb = ram.unwrap();
            assert!(gb > 0.5, "RAM should be at least 0.5 GB, got {}", gb);
            assert!(gb < 4096.0, "RAM should be under 4 TB, got {}", gb);
        }
        // Suppress unused variable warning on other platforms
        let _ = ram;
    }

    #[tokio::test]
    async fn test_test_model_inference_ollama_parse() {
        // Verify the Ollama response parsing logic by simulating what test_model_inference does
        let json = r#"{"message":{"role":"assistant","content":"{\"ok\": true}"}}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let content = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        assert_eq!(content, "{\"ok\": true}");
    }

    #[tokio::test]
    async fn test_test_model_inference_openai_parse() {
        // Verify the OpenAI response parsing logic
        let json = r#"{"choices":[{"message":{"content":"{\"ok\": true}"}}]}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let content = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        assert_eq!(content, "{\"ok\": true}");
    }

    #[tokio::test]
    async fn test_test_model_inference_empty_choices() {
        // OpenAI response with empty choices array
        let json = r#"{"choices":[]}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let content = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        assert_eq!(content, "");
    }

    #[tokio::test]
    async fn test_detect_context_window_from_parameters() {
        // Simulate Ollama /api/show response with parameters field
        let json = r#"{"parameters":"stop [INST]\nstop [/INST]\nnum_ctx 4096\nrepeat_penalty 1.1"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut result = None;
        if let Some(params) = value.get("parameters").and_then(|p| p.as_str()) {
            for line in params.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("num_ctx") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        result = val.parse().ok();
                    }
                }
            }
        }
        assert_eq!(result, Some(4096));
    }

    #[tokio::test]
    async fn test_detect_context_window_from_model_info() {
        // Simulate Ollama /api/show response with model_info field
        let json = r#"{"model_info":{"llama.context_length":8192}}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut result = None;
        if let Some(info) = value.get("model_info") {
            for key in &[
                "context_length",
                "llama.context_length",
                "general.context_length",
            ] {
                if let Some(ctx) = info.get(*key).and_then(|v| v.as_u64()) {
                    result = Some(ctx as usize);
                    break;
                }
            }
        }
        assert_eq!(result, Some(8192));
    }

    #[tokio::test]
    async fn test_detect_context_window_no_data() {
        // Response with neither parameters nor model_info
        let json = r#"{"license":"MIT","modelfile":"..."}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut result = None;
        if let Some(params) = value.get("parameters").and_then(|p| p.as_str()) {
            for line in params.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("num_ctx") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        result = val.parse().ok();
                    }
                }
            }
        }
        if result.is_none() {
            if let Some(info) = value.get("model_info") {
                for key in &[
                    "context_length",
                    "llama.context_length",
                    "general.context_length",
                ] {
                    if let Some(ctx) = info.get(*key).and_then(|v| v.as_u64()) {
                        result = Some(ctx as usize);
                        break;
                    }
                }
            }
        }
        assert_eq!(result, None);
    }
}
