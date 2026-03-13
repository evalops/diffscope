use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::core;
use crate::output::{format_diff_as_unified, OutputFormat};
use crate::review::review_diff_content;

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
