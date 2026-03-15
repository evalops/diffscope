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
    println!(
        "  Primary Provider: {}",
        config
            .resolved_provider_for_role(crate::config::ModelRole::Primary)
            .provider
            .as_deref()
            .unwrap_or("(auto-detect)")
    );
    let validation_issues = config.validation_issues();
    if !validation_issues.is_empty() {
        println!("  Validation:");
        for issue in validation_issues {
            println!(
                "    - {}: {}",
                match issue.level {
                    crate::config::ConfigValidationIssueLevel::Warning => "warning",
                    crate::config::ConfigValidationIssueLevel::Error => "error",
                },
                issue.message,
            );
        }
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
