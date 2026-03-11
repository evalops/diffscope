use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;
use tracing::info;

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::review::review_diff_content_with_repo;

#[derive(Subcommand)]
pub enum GitCommands {
    Uncommitted,
    Staged,
    Branch {
        #[arg(help = "Base branch/ref (defaults to repo default)")]
        base: Option<String>,
    },
    Suggest,
    PrTitle,
}

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

async fn suggest_commit_message(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let diff_content = git.get_staged_diff()?;

    if diff_content.is_empty() {
        println!("No staged changes found. Stage your changes with 'git add' first.");
        return Ok(());
    }

    // Use Fast model for commit message suggestion (lightweight task)
    let model_config = config.to_model_config_for_role(config::ModelRole::Fast);

    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_commit_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(500),
    };

    let response = adapter.complete(request).await?;
    let commit_message = core::CommitPromptBuilder::extract_commit_message(&response.content);

    println!("\nSuggested commit message:");
    println!("{}", commit_message);

    if commit_message.len() > 72 {
        println!(
            "\n⚠️  Warning: Commit message exceeds 72 characters ({})",
            commit_message.len()
        );
    }

    Ok(())
}

async fn suggest_pr_title(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let base_branch = git
        .get_default_branch()
        .unwrap_or_else(|_| "main".to_string());
    let diff_content = git.get_branch_diff(&base_branch)?;

    if diff_content.is_empty() {
        println!("No changes found compared to {} branch.", base_branch);
        return Ok(());
    }

    // Use Fast model for PR title suggestion (lightweight task)
    let model_config = config.to_model_config_for_role(config::ModelRole::Fast);

    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_pr_title_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(200),
    };

    let response = adapter.complete(request).await?;

    let title = extract_title_from_response(&response.content);

    println!("\nSuggested PR title:");
    println!("{}", title);

    if title.len() > 65 {
        println!(
            "\n⚠️  Warning: PR title exceeds 65 characters ({})",
            title.len()
        );
    }

    Ok(())
}

fn extract_title_from_response(content: &str) -> String {
    if let Some(start) = content.find("<title>") {
        let after_tag = start + 7;
        if let Some(end) = content[after_tag..].find("</title>") {
            content[after_tag..after_tag + end].trim().to_string()
        } else {
            content.trim().to_string()
        }
    } else {
        content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title_normal() {
        let content = "<title>Fix login bug</title>";
        assert_eq!(extract_title_from_response(content), "Fix login bug");
    }

    #[test]
    fn test_extract_title_malformed_closing_before_opening() {
        // Malformed: closing tag appears before opening tag
        // This should NOT panic
        let content = "Some text</title> more <title>Real Title</title>";
        let title = extract_title_from_response(content);
        assert!(!title.is_empty());
    }

    #[test]
    fn test_extract_title_no_tags() {
        let content = "Just a plain title\nSecond line";
        assert_eq!(extract_title_from_response(content), "Just a plain title");
    }
}
