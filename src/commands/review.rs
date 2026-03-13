use anyhow::Result;
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::core;
use crate::output::{format_diff_as_unified, OutputFormat};
use crate::review::{review_diff_content, review_diff_content_raw, review_diff_content_with_repo};

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

pub(crate) async fn load_review_input(diff_path: Option<PathBuf>) -> Result<(PathBuf, String)> {
    let repo_root = core::GitIntegration::new(".")
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or_else(|| PathBuf::from("."));

    let diff_content = if let Some(path) = diff_path {
        tokio::fs::read_to_string(path).await?
    } else if std::io::stdin().is_terminal() {
        if let Ok(git) = core::GitIntegration::new(".") {
            let diff = git.get_uncommitted_diff()?;
            if diff.is_empty() {
                println!("No changes found");
                return Ok((repo_root, String::new()));
            }
            diff
        } else {
            println!("No diff provided and not in a git repository.");
            return Ok((repo_root, String::new()));
        }
    } else {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    Ok((repo_root, diff_content))
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

    // Use the parse_text_diff function to create a UnifiedDiff
    let diff = core::DiffParser::parse_text_diff(&old_content, &new_content, new_file.clone())?;

    // Convert the diff to a string format for the review process
    let diff_string = format!(
        "--- {}\n+++ {}\n{}",
        old_file.display(),
        new_file.display(),
        format_diff_as_unified(&diff)
    );

    review_diff_content(&diff_string, config, format).await
}
