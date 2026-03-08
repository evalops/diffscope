use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;
use crate::core::offline::{self, OfflineConfig, OfflineModelManager};

pub async fn doctor_command(config: Config) -> Result<()> {
    println!("DiffScope Doctor");
    println!("================\n");

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

            // Usage hint
            let model_flag = if endpoint_type == "ollama" {
                format!("ollama:{}", recommended.name)
            } else {
                recommended.name.clone()
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
}
