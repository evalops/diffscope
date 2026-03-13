use anyhow::Result;

use crate::config::Config;

use super::super::display::print_endpoint_models;
use super::super::probe::EndpointProbe;
use super::super::recommend::inspect_recommended_model;

pub(in super::super) async fn run_recommendation_flow(
    config: &Config,
    base_url: &str,
    endpoint: &EndpointProbe,
) -> Result<()> {
    println!("reachable ({})", endpoint.reachable_label);
    print_endpoint_models(endpoint.endpoint_type, &endpoint.models);
    inspect_recommended_model(config, base_url, endpoint).await
}
