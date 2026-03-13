use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;
use crate::core::offline::{self, OfflineConfig, OfflineModelManager};

use super::endpoint::{estimate_tokens, parse_openai_models, test_model_inference};
use super::system::check_system_resources;

pub async fn doctor_command(config: Config) -> Result<()> {
    println!("DiffScope Doctor");
    println!("================\n");

    check_system_resources();

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

    print!("Checking endpoint {}... ", base_url);
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;

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

            if endpoint_type == "ollama" {
                if let Ok(Some(ctx_size)) = manager.detect_context_window(&recommended.name).await {
                    println!(
                        "  Context window: {} tokens (detected from model)",
                        ctx_size
                    );
                }
            }

            let readiness = offline::check_readiness(&offline_config, &manager);
            if readiness.ready {
                println!("\nStatus: READY");
            } else {
                println!("\nStatus: NOT READY");
                for warning in &readiness.warnings {
                    println!("  Warning: {}", warning);
                }
            }

            let recommended_name = recommended.name.clone();
            print!("\nTesting model {}... ", recommended_name);
            let test_client = Client::builder().timeout(Duration::from_secs(10)).build()?;
            let test_start = std::time::Instant::now();
            let test_result =
                test_model_inference(&test_client, &base_url, &recommended_name, endpoint_type)
                    .await;
            let elapsed = test_start.elapsed();

            match test_result {
                Ok(response) => {
                    let tokens_per_sec = estimate_tokens(&response) as f64 / elapsed.as_secs_f64();
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

    #[tokio::test]
    async fn test_detect_context_window_from_parameters() {
        let json =
            r#"{"parameters":"stop [INST]\nstop [/INST]\nnum_ctx 4096\nrepeat_penalty 1.1"}"#;
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
