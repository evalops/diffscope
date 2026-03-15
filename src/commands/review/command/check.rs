use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::review::review_diff_content_with_repo;

pub async fn check_command(
    path: PathBuf,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    info!("Checking repository at: {}", path.display());
    info!("Using generation model: {}", config.generation_model_name());

    let git = core::GitIntegration::new(&path)?;
    let diff_content = git.get_uncommitted_diff()?;
    if diff_content.is_empty() {
        println!("No changes found in {}", path.display());
        return Ok(());
    }

    let repo_root = git.workdir().unwrap_or(path);
    review_diff_content_with_repo(&diff_content, config, format, &repo_root).await
}
