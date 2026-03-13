use anyhow::Result;
use std::path::Path;

use crate::config;

#[path = "orchestrate/dag.rs"]
mod dag;

use super::types::{ProgressCallback, ReviewResult};
use dag::execute_review_pipeline_dag;

pub(crate) fn describe_review_pipeline_graph() -> crate::core::dag::DagGraphContract {
    dag::describe_review_pipeline_graph()
}

pub(super) async fn review_diff_content_raw_inner(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    execute_review_pipeline_dag(diff_content, config, repo_path, on_progress).await
}
