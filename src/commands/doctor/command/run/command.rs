use anyhow::Result;

use crate::config::Config;

use super::super::super::system::check_system_resources;
use super::super::display::{print_configuration, print_header, print_unreachable};
use super::endpoint::{configured_base_url, discover_endpoint};
use super::recommendation::run_recommendation_flow;

pub async fn doctor_command(config: Config) -> Result<()> {
    print_header();
    check_system_resources();
    print_configuration(&config);

    let base_url = configured_base_url(&config);

    print!("Checking endpoint {}... ", base_url);
    let Some(endpoint) = discover_endpoint(&base_url).await? else {
        return print_unreachable(&base_url);
    };

    run_recommendation_flow(&config, &base_url, &endpoint).await
}
