use anyhow::Result;
use reqwest::Client;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::core::offline::{self, OfflineConfig, OfflineModelManager};

use super::super::endpoint::{estimate_tokens, test_model_inference};
use super::display::{
    print_inference_failure, print_inference_success, print_recommended_model_summary, print_usage,
};
use super::probe::EndpointProbe;

pub(super) async fn inspect_recommended_model(
    config: &Config,
    base_url: &str,
    endpoint: &EndpointProbe,
) -> Result<()> {
    if endpoint.models.is_empty() {
        return Ok(());
    }

    let mut manager = OfflineModelManager::new(base_url);
    manager.set_models(endpoint.models.clone());

    let Some(recommended) = manager.recommend_review_model() else {
        return Ok(());
    };

    let recommended_name = recommended.name.clone();
    let offline_config = OfflineConfig {
        model_name: recommended_name.clone(),
        base_url: base_url.to_string(),
        context_window: config.context_window.unwrap_or(8192),
        max_tokens: config.max_tokens,
        ..Default::default()
    };
    let detected_context_window = if endpoint.is_ollama() {
        manager
            .detect_context_window(&recommended_name)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let readiness = offline::check_readiness(&offline_config, &manager);

    print_recommended_model_summary(
        recommended,
        offline_config.estimated_ram_mb(),
        detected_context_window,
        &readiness,
    );

    print!("\nTesting model {}... ", recommended_name);
    let test_client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let started_at = Instant::now();
    match test_model_inference(
        &test_client,
        base_url,
        &recommended_name,
        endpoint.endpoint_type,
    )
    .await
    {
        Ok(response) => {
            let elapsed = started_at.elapsed();
            let tokens_per_sec = estimate_tokens(&response) as f64 / elapsed.as_secs_f64();
            print_inference_success(elapsed, tokens_per_sec);
        }
        Err(error) => print_inference_failure(&error),
    }

    let model_flag = endpoint.model_flag(&recommended_name);
    print_usage(base_url, &model_flag);
    Ok(())
}
