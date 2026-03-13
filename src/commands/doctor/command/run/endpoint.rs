use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

use crate::config::Config;

use super::super::probe::{probe_endpoint, EndpointProbe};

pub(in super::super) fn configured_base_url(config: &Config) -> String {
    config
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".to_string())
}

pub(in super::super) async fn discover_endpoint(base_url: &str) -> Result<Option<EndpointProbe>> {
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    probe_endpoint(&client, base_url).await
}
