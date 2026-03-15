use anyhow::Result;
use std::path::Path;

use crate::config;
use crate::output::OutputFormat;

#[path = "pipeline/chunking.rs"]
mod chunking;
#[path = "pipeline/comments.rs"]
mod comments;
#[path = "pipeline/context.rs"]
mod context;
#[path = "pipeline/contracts.rs"]
mod contracts;
#[path = "pipeline/execution.rs"]
mod execution;
#[path = "pipeline/file_context.rs"]
mod file_context;
#[path = "pipeline/guidance.rs"]
mod guidance;
#[path = "pipeline/orchestrate.rs"]
mod orchestrate;
#[path = "pipeline/postprocess.rs"]
mod postprocess;
#[path = "pipeline/prepare.rs"]
mod prepare;
#[path = "pipeline/repo_support.rs"]
mod repo_support;
#[path = "pipeline/request.rs"]
mod request;
#[path = "pipeline/services.rs"]
mod services;
#[path = "pipeline/session.rs"]
mod session;
#[path = "pipeline/types.rs"]
mod types;

use chunking::maybe_review_chunked_diff_content;

pub use comments::{filter_comments_for_diff, is_line_in_diff};
pub use context::{build_symbol_index, extract_symbols_from_diff};
pub use guidance::build_review_guidance;
pub(crate) use orchestrate::describe_review_pipeline_graph;
pub(crate) use postprocess::describe_review_postprocess_graph;
pub(crate) use types::{AgentActivity, FileMetric, ReviewResult};
pub use types::{ProgressCallback, ProgressUpdate};

pub async fn review_diff_content(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    review_diff_content_with_repo(diff_content, config, format, Path::new(".")).await
}

pub async fn review_diff_content_with_repo(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
    repo_path: &Path,
) -> Result<()> {
    let rule_priority = config.rule_priority.clone();
    let result = review_diff_content_raw(diff_content, config, repo_path).await?;
    crate::output::output_comments(&result.comments, None, format, &rule_priority).await
}

pub async fn review_diff_content_raw(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
) -> Result<ReviewResult> {
    review_diff_content_raw_internal(diff_content, config, repo_path, None, None).await
}

pub async fn review_diff_content_raw_with_verification_reuse(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    verification_reuse_cache: crate::review::verification::VerificationReuseCache,
) -> Result<ReviewResult> {
    review_diff_content_raw_internal(
        diff_content,
        config,
        repo_path,
        None,
        Some(verification_reuse_cache),
    )
    .await
}

#[tracing::instrument(name = "review_pipeline", skip(diff_content, config, repo_path, on_progress), fields(diff_bytes = diff_content.len(), model = %config.model))]
pub async fn review_diff_content_raw_with_progress(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    review_diff_content_raw_internal(diff_content, config, repo_path, on_progress, None).await
}

pub async fn review_diff_content_raw_with_progress_and_verification_reuse(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
    verification_reuse_cache: crate::review::verification::VerificationReuseCache,
) -> Result<ReviewResult> {
    review_diff_content_raw_internal(
        diff_content,
        config,
        repo_path,
        on_progress,
        Some(verification_reuse_cache),
    )
    .await
}

async fn review_diff_content_raw_internal(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
    verification_reuse_cache: Option<crate::review::verification::VerificationReuseCache>,
) -> Result<ReviewResult> {
    let verification_reuse_cache = verification_reuse_cache.unwrap_or_default();

    if let Some(result) = maybe_review_chunked_diff_content(
        diff_content,
        config.clone(),
        repo_path,
        on_progress.clone(),
        verification_reuse_cache.clone(),
    )
    .await?
    {
        return Ok(result);
    }

    orchestrate::review_diff_content_raw_inner(
        diff_content,
        config,
        repo_path,
        on_progress,
        verification_reuse_cache,
    )
    .await
}
