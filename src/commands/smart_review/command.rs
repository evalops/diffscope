use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::{build_change_walkthrough, format_smart_review_output};
use crate::review;

use super::summary::build_pr_summary;

pub async fn smart_review_command(
    config: config::Config,
    diff_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!(
        "Starting smart review analysis with model: {}",
        config.model
    );

    let (repo_root, diff_content) = super::super::review::load_review_input(diff_path).await?;
    if diff_content.trim().is_empty() {
        return Ok(());
    }

    let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
    info!("Parsed {} file diffs", diffs.len());
    let walkthrough = build_change_walkthrough(&diffs);

    let model_config = config.to_model_config();
    let adapter = adapters::llm::create_adapter(&model_config)?;
    let pr_summary = build_pr_summary(&config, &repo_root, &diffs, adapter.as_ref()).await?;

    let review_result =
        review::review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;
    let processed_comments = review_result.comments;

    let summary = core::CommentSynthesizer::generate_summary(&processed_comments);
    let output = format_smart_review_output(
        &processed_comments,
        &summary,
        pr_summary.as_ref(),
        &walkthrough,
        &config.rule_priority,
    );

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
    } else {
        println!("{}", output);
    }

    Ok(())
}
