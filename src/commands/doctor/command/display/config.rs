use anyhow::Result;

use crate::config::Config;

pub(in super::super) fn print_header() {
    println!("DiffScope Doctor");
    println!("================\n");
}

pub(in super::super) fn print_configuration(config: &Config) {
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
        println!("  Context:  {cw} tokens");
    }
    println!();
}

pub(in super::super) fn print_unreachable(base_url: &str) -> Result<()> {
    println!("UNREACHABLE");
    println!("\nCannot reach {base_url}. Make sure your LLM server is running.");
    println!("\nQuick start:");
    println!("  Ollama:    ollama serve");
    println!("  vLLM:      vllm serve <model>");
    println!("  LM Studio: Start the app and enable the local server");
    Ok(())
}
