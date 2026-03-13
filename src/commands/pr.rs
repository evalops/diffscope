use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

#[path = "pr/comments.rs"]
mod comments;
#[path = "pr/gh.rs"]
mod gh;

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::review;

use comments::post_review_comments;
use gh::{fetch_pr_diff, fetch_pr_metadata, resolve_pr_number};

pub async fn pr_command(
    number: Option<u32>,
    repo: Option<String>,
    post_comments: bool,
    summary: bool,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    let pr_number = resolve_pr_number(number, repo.as_deref())?;

    info!("Reviewing PR #{}", pr_number);

    let git = core::GitIntegration::new(".")?;
    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));
    if let Ok(branch) = git.get_current_branch() {
        info!("Current branch: {}", branch);
    }
    if let Ok(Some(remote)) = git.get_remote_url() {
        info!("Remote URL: {}", remote);
    }

    let diff_content = fetch_pr_diff(&pr_number, repo.as_deref())?;

    if diff_content.is_empty() {
        println!("No changes in PR");
        return Ok(());
    }

    if summary {
        let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
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
        return Ok(());
    }

    let review_result =
        review::review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;
    let comments = review_result.comments;

    if post_comments {
        info!("Posting {} comments to PR", comments.len());
        let metadata = fetch_pr_metadata(&pr_number, repo.as_deref())?;
        let stats = post_review_comments(
            &pr_number,
            repo.as_deref(),
            &metadata,
            &comments,
            &config.rule_priority,
        )?;

        println!(
            "Posted {} comments to PR #{} (inline: {}, fallback: {}, summary: updated)",
            comments.len(),
            pr_number,
            stats.inline_posted,
            stats.fallback_posted
        );
    } else {
        crate::output::output_comments(&comments, None, format, &config.rule_priority).await?;
    }

    Ok(())
}
