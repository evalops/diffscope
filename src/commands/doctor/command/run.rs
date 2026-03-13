use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;

use super::super::system::check_system_resources;
use super::display::{print_configuration, print_endpoint_models, print_header, print_unreachable};
use super::probe::probe_endpoint;
use super::recommend::inspect_recommended_model;

pub async fn doctor_command(config: Config) -> Result<()> {
    print_header();
    check_system_resources();
    print_configuration(&config);

    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".to_string());

    print!("Checking endpoint {}... ", base_url);
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    let Some(endpoint) = probe_endpoint(&client, &base_url).await? else {
        return print_unreachable(&base_url);
    };

    println!("reachable ({})", endpoint.reachable_label);
    print_endpoint_models(endpoint.endpoint_type, &endpoint.models);
    inspect_recommended_model(&config, &base_url, &endpoint).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn doctor_config_defaults() {
        let config = Config::default();
        assert!(config.adapter.is_none());
        assert!(config.context_window.is_none());
    }

    #[test]
    fn test_detect_context_window_from_parameters() {
        let json =
            r#"{"parameters":"stop [INST]\nstop [/INST]\nnum_ctx 4096\nrepeat_penalty 1.1"}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse_context_window(&value), Some(4096));
    }

    #[test]
    fn test_detect_context_window_from_model_info() {
        let json = r#"{"model_info":{"llama.context_length":8192}}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse_context_window(&value), Some(8192));
    }

    #[test]
    fn test_detect_context_window_no_data() {
        let json = r#"{"license":"MIT","modelfile":"..."}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse_context_window(&value), None);
    }

    fn parse_context_window(value: &Value) -> Option<usize> {
        if let Some(params) = value
            .get("parameters")
            .and_then(|parameters| parameters.as_str())
        {
            for line in params.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("num_ctx") {
                    if let Some(raw_value) = trimmed.split_whitespace().nth(1) {
                        if let Ok(parsed) = raw_value.parse() {
                            return Some(parsed);
                        }
                    }
                }
            }
        }

        let info = value.get("model_info")?;
        for key in &[
            "context_length",
            "llama.context_length",
            "general.context_length",
        ] {
            if let Some(ctx) = info.get(*key).and_then(|value| value.as_u64()) {
                return Some(ctx as usize);
            }
        }

        None
    }
}
