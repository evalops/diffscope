use anyhow::Result;
use tracing::info;

#[path = "pr/comments.rs"]
mod comments;
#[path = "pr/context.rs"]
mod context;
#[path = "pr/gh.rs"]
mod gh;
#[path = "pr/readiness.rs"]
mod readiness;
#[path = "pr/review_flow.rs"]
mod review_flow;
#[path = "pr/summary_flow.rs"]
mod summary_flow;

use crate::config;
use crate::output::OutputFormat;

use context::prepare_pr_context;
use readiness::run_pr_readiness_flow;
use review_flow::run_pr_review_flow;
use summary_flow::run_pr_summary_flow;

pub async fn pr_command(
    number: Option<u32>,
    repo: Option<String>,
    post_comments: bool,
    summary: bool,
    readiness: bool,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    if readiness {
        return run_pr_readiness_flow(number, repo.as_deref(), config, format).await;
    }

    let context = prepare_pr_context(number, repo.as_deref())?;

    info!("Reviewing PR #{}", context.pr_number);

    if context.diff_content.is_empty() {
        println!("No changes in PR");
        return Ok(());
    }

    if summary {
        run_pr_summary_flow(&config, &context.diff_content).await?;
        return Ok(());
    }

    run_pr_review_flow(&context, repo.as_deref(), post_comments, &config, format).await
}
