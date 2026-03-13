use anyhow::Result;
use std::path::Path;

use crate::config;

use super::contracts::ReviewExecutionContext;
use super::execution::execute_review_jobs;
use super::postprocess::run_postprocess;
use super::prepare::prepare_file_review_jobs;
use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::{ProgressCallback, ReviewResult};

pub(super) async fn review_diff_content_raw_inner(
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
