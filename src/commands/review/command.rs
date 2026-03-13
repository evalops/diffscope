use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::core;
use crate::output::{format_diff_as_unified, OutputFormat};
use crate::review::{review_diff_content, review_diff_content_raw, review_diff_content_with_repo};

use super::load_review_input;

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

pub async fn check_command(
    path: PathBuf,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    info!("Checking repository at: {}", path.display());
    info!("Using model: {}", config.model);

    let git = core::GitIntegration::new(&path)?;
    let diff_content = git.get_uncommitted_diff()?;
    if diff_content.is_empty() {
        println!("No changes found in {}", path.display());
        return Ok(());
    }

    let repo_root = git.workdir().unwrap_or(path);
    review_diff_content_with_repo(&diff_content, config, format, &repo_root).await
}

pub async fn compare_command(
    old_file: PathBuf,
    new_file: PathBuf,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    info!(
        "Comparing files: {} vs {}",
        old_file.display(),
        new_file.display()
    );

    let old_content = tokio::fs::read_to_string(&old_file).await?;
    let new_content = tokio::fs::read_to_string(&new_file).await?;

    let diff = core::DiffParser::parse_text_diff(&old_content, &new_content, new_file.clone())?;
    let diff_string = format!(
        "--- {}\n+++ {}\n{}",
        old_file.display(),
        new_file.display(),
        format_diff_as_unified(&diff)
    );

    review_diff_content(&diff_string, config, format).await
}
