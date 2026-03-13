use anyhow::Result;

use crate::adapters;
use crate::config;
use crate::core;

pub(super) async fn run_pr_summary_flow(config: &config::Config, diff_content: &str) -> Result<()> {
    let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
    let git = core::GitIntegration::new(".")?;

    let fast_config = config.to_model_config_for_role(config::ModelRole::Fast);
    let adapter = adapters::llm::create_adapter(&fast_config)?;
    let options = core::SummaryOptions {
        include_diagram: config.smart_review_diagram,
    };
    let pr_summary = core::PRSummaryGenerator::generate_summary_with_options(
        &diffs,
        &git,
        adapter.as_ref(),
        options,
    )
    .await?;

    println!("{}", pr_summary.to_markdown());
    Ok(())
}
