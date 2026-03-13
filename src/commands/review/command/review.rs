use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::output::OutputFormat;
use crate::review::review_diff_content_raw;

use super::super::load_review_input;

pub async fn review_command(
    config: config::Config,
    diff_path: Option<PathBuf>,
    patch: bool,
    output_path: Option<PathBuf>,
    format: OutputFormat,
) -> Result<()> {
    info!("Starting diff review with model: {}", config.model);

    let (repo_root, diff_content) = load_review_input(diff_path).await?;
    if diff_content.trim().is_empty() {
        return Ok(());
    }

    let result = review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;
    let effective_format = if patch { OutputFormat::Patch } else { format };
    crate::output::output_comments(
        &result.comments,
        output_path,
        effective_format,
        &config.rule_priority,
    )
    .await?;

    Ok(())
}
