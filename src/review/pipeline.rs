use anyhow::Result;
use std::path::Path;

use crate::config;
use crate::output::OutputFormat;

#[path = "pipeline/comments.rs"]
mod comments;
#[path = "pipeline/context.rs"]
mod context;
#[path = "pipeline/contracts.rs"]
mod contracts;
#[path = "pipeline/execution.rs"]
mod execution;
#[path = "pipeline/guidance.rs"]
mod guidance;
#[path = "pipeline/postprocess.rs"]
mod postprocess;
#[path = "pipeline/prepare.rs"]
mod prepare;
#[path = "pipeline/request.rs"]
mod request;
#[path = "pipeline/session.rs"]
mod session;
#[path = "pipeline/types.rs"]
mod types;

use contracts::ReviewExecutionContext;
use execution::execute_review_jobs;
use postprocess::run_postprocess;
use prepare::prepare_file_review_jobs;
use session::{chunk_diff_for_context, should_optimize_for_local, PipelineServices, ReviewSession};

pub use comments::{filter_comments_for_diff, is_line_in_diff};
pub use context::{build_symbol_index, extract_symbols_from_diff};
pub use guidance::build_review_guidance;
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
    review_diff_content_raw_with_progress(diff_content, config, repo_path, None).await
}

#[tracing::instrument(name = "review_pipeline", skip(diff_content, config, repo_path, on_progress), fields(diff_bytes = diff_content.len(), model = %config.model))]
pub async fn review_diff_content_raw_with_progress(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    if should_optimize_for_local(&config) {
        let context_budget = config.context_window.unwrap_or(8192);
        let max_diff_chars = (context_budget * 2 / 5).max(1000);
        let chunks = chunk_diff_for_context(diff_content, max_diff_chars);
        if chunks.len() > 1 {
            eprintln!(
                "Diff split into {} chunks for local model context window",
                chunks.len()
            );
            let mut merged = ReviewResult::default();
            for (index, chunk) in chunks.iter().enumerate() {
                eprintln!("Processing chunk {}/{}...", index + 1, chunks.len());
                match review_diff_content_raw_inner(
                    chunk,
                    config.clone(),
                    repo_path,
                    on_progress.clone(),
                )
                .await
                {
                    Ok(chunk_result) => {
                        merged.comments.extend(chunk_result.comments);
                        merged.total_prompt_tokens += chunk_result.total_prompt_tokens;
                        merged.total_completion_tokens += chunk_result.total_completion_tokens;
                        merged.total_tokens += chunk_result.total_tokens;
                        merged.file_metrics.extend(chunk_result.file_metrics);
                        merged.convention_suppressed_count +=
                            chunk_result.convention_suppressed_count;
                        for (pass, count) in chunk_result.comments_by_pass {
                            *merged.comments_by_pass.entry(pass).or_insert(0) += count;
                        }
                        merged.hotspots.extend(chunk_result.hotspots);
                    }
                    Err(error) => {
                        eprintln!("Warning: chunk {} failed: {}", index + 1, error);
                    }
                }
            }
            return Ok(merged);
        }
    }

    review_diff_content_raw_inner(diff_content, config, repo_path, on_progress).await
}

async fn review_diff_content_raw_inner(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    let services = PipelineServices::new(config, repo_path).await?;
    let mut session = ReviewSession::new(diff_content, &services, on_progress).await?;

    let prepared = prepare_file_review_jobs(&services, &mut session).await?;
    let execution = execute_review_jobs(
        prepared.jobs,
        ReviewExecutionContext {
            services: &services,
            session: &session,
            initial_comments: prepared.all_comments,
            files_completed: prepared.files_completed,
            files_skipped: prepared.files_skipped,
        },
    )
    .await?;

    run_postprocess(execution, &services, &mut session).await
}
