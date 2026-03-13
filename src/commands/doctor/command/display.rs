use anyhow::Result;
use std::time::Duration;

use crate::config::Config;
use crate::core::offline::{LocalModel, ReadinessCheck};

pub(super) fn print_header() {
    println!("DiffScope Doctor");
    println!("================\n");
}

pub(super) fn print_configuration(config: &Config) {
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
}

pub(super) fn print_endpoint_models(endpoint_type: &str, models: &[LocalModel]) {
    println!("\nEndpoint type: {}", endpoint_type);
    println!("\nAvailable models ({}):", models.len());
    if models.is_empty() {
        println!("  (none found)");
        if endpoint_type == "ollama" {
            println!("\n  Pull a model: ollama pull codellama");
        }
        return;
    }

    for model in models {
        println!("  - {}{}", model.name, format_model_size_info(model));
    }
}

pub(super) fn print_recommended_model_summary(
    recommended: &LocalModel,
    estimated_ram_mb: usize,
    detected_context_window: Option<usize>,
    readiness: &ReadinessCheck,
) {
    println!("\nRecommended for code review: {}", recommended.name);
    println!("  Estimated RAM: ~{}MB", estimated_ram_mb);

    if let Some(ctx_size) = detected_context_window {
        println!(
            "  Context window: {} tokens (detected from model)",
            ctx_size
        );
    }

    if readiness.ready {
        println!("\nStatus: READY");
    } else {
        println!("\nStatus: NOT READY");
        for warning in &readiness.warnings {
            println!("  Warning: {}", warning);
        }
    }
}

pub(super) fn print_inference_success(elapsed: Duration, tokens_per_sec: f64) {
    println!(
        "OK ({:.1}s, ~{:.0} tok/s)",
        elapsed.as_secs_f64(),
        tokens_per_sec
    );
    if tokens_per_sec < 2.0 {
        println!("  Warning: Very slow inference. Consider a smaller/quantized model.");
    }
}

pub(super) fn print_inference_failure(error: &impl std::fmt::Display) {
    println!("FAILED");
    println!("  Error: {}", error);
    println!("  The model may still be loading. Try again in a moment.");
}

pub(super) fn print_usage(base_url: &str, model_flag: &str) {
    println!("\nUsage:");
    println!(
        "  git diff | diffscope review --base-url {} --model {}",
        base_url, model_flag
    );
}

pub(super) fn print_unreachable(base_url: &str) -> Result<()> {
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

fn format_model_size_info(model: &LocalModel) -> String {
    if model.size_mb == 0 {
        return String::new();
    }

    format!(" ({}MB", model.size_mb)
        + &model
            .quantization
            .as_ref()
            .map(|quantization| format!(", {}", quantization))
            .unwrap_or_default()
        + ")"
}
