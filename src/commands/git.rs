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

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

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

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

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

    // Extract title from response
    let title = if let Some(start) = response.content.find("<title>") {
        if let Some(end) = response.content.find("</title>") {
            response.content[start + 7..end].trim().to_string()
        } else {
            response.content.trim().to_string()
        }
    } else {
        // Fallback: take the first non-empty line
        response
            .content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    };

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
