use anyhow::Result;
use tracing::info;

use crate::config;
use crate::output::OutputFormat;
use crate::review;

use super::comments::post_review_comments;
use super::context::PrCommandContext;
use super::gh::fetch_pr_metadata;

pub(super) async fn run_pr_review_flow(
    context: &PrCommandContext,
    repo: Option<&str>,
    post_comments: bool,
    config: &config::Config,
    format: OutputFormat,
) -> Result<()> {
    let review_result =
        review::review_diff_content_raw(&context.diff_content, config.clone(), &context.repo_root)
            .await?;
    let comments = review_result.comments;

    if post_comments {
        post_pr_review_comments(context, repo, &comments, config)?;
    } else {
        crate::output::output_comments(&comments, None, format, &config.rule_priority).await?;
    }

    Ok(())
}

fn post_pr_review_comments(
    context: &PrCommandContext,
    repo: Option<&str>,
    comments: &[crate::core::Comment],
    config: &config::Config,
) -> Result<()> {
    info!("Posting {} comments to PR", comments.len());
    let metadata = fetch_pr_metadata(&context.pr_number, repo)?;
    let stats = post_review_comments(
        &context.pr_number,
        repo,
        &metadata,
        comments,
        &config.rule_priority,
    )?;

    println!(
        "Posted {} comments to PR #{} (inline: {}, fallback: {}, summary: updated)",
        comments.len(),
        context.pr_number,
        stats.inline_posted,
        stats.fallback_posted
    );

    Ok(())
}
