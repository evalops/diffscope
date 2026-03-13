use anyhow::Result;
use reqwest::Client;

use crate::core::offline::{LocalModel, OfflineModelManager};

use super::super::endpoint::parse_openai_models;

pub(super) struct EndpointProbe {
    pub(super) endpoint_type: &'static str,
    pub(super) reachable_label: &'static str,
    pub(super) models: Vec<LocalModel>,
}

impl EndpointProbe {
    pub(super) fn is_ollama(&self) -> bool {
        self.endpoint_type == "ollama"
    }

    pub(super) fn model_flag(&self, model_name: &str) -> String {
        if self.is_ollama() {
            format!("ollama:{}", model_name)
        } else {
            model_name.to_string()
        }
    }
}

pub(super) async fn probe_endpoint(
    client: &Client,
    base_url: &str,
) -> Result<Option<EndpointProbe>> {
    if let Some(models) = probe_ollama_endpoint(client, base_url).await? {
        return Ok(Some(EndpointProbe {
            endpoint_type: "ollama",
            reachable_label: "Ollama",
            models,
        }));
    }

    if let Some(models) = probe_openai_endpoint(client, base_url).await? {
        return Ok(Some(EndpointProbe {
            endpoint_type: "openai-compatible",
            reachable_label: "OpenAI-compatible",
            models,
        }));
    }

    Ok(None)
}

async fn probe_ollama_endpoint(client: &Client, base_url: &str) -> Result<Option<Vec<LocalModel>>> {
    let url = format!("{}/api/tags", base_url);
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => return Ok(None),
    };
    if !response.status().is_success() {
        return Ok(None);
    }

    let body = response.text().await?;
    Ok(Some(
        OfflineModelManager::parse_model_list(&body).unwrap_or_default(),
    ))
}

async fn probe_openai_endpoint(client: &Client, base_url: &str) -> Result<Option<Vec<LocalModel>>> {
    let url = format!("{}/v1/models", base_url);
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => return Ok(None),
    };
    if !response.status().is_success() {
        return Ok(None);
    }

    let body = response.text().await?;
    let mut models = Vec::new();
    parse_openai_models(&body, &mut models);
    Ok(Some(models))
}
