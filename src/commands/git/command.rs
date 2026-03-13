use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::review::review_diff_content_with_repo;

use super::suggest::{suggest_commit_message, suggest_pr_title};
use super::GitCommands;

pub async fn git_command(
    command: GitCommands,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    let git = core::GitIntegration::new(".")?;

    let diff_content = match command {
        GitCommands::Uncommitted => {
            info!("Analyzing uncommitted changes");
            git.get_uncommitted_diff()?
        }
        GitCommands::Staged => {
            info!("Analyzing staged changes");
            git.get_staged_diff()?
        }
        GitCommands::Branch { base } => {
            let base_branch = base.unwrap_or_else(|| {
                git.get_default_branch()
                    .unwrap_or_else(|_| "main".to_string())
            });
            core::validate_ref_name(&base_branch)?;
            info!("Analyzing changes from branch: {}", base_branch);
            git.get_branch_diff(&base_branch)?
        }
        GitCommands::Suggest => {
            return suggest_commit_message(config).await;
        }
        GitCommands::PrTitle => {
            return suggest_pr_title(config).await;
        }
    };

    if diff_content.is_empty() {
        println!("No changes found");
        return Ok(());
    }

    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));
    review_diff_content_with_repo(&diff_content, config, format, &repo_root).await
}
