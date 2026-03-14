use anyhow::Result;

use crate::core;

use super::super::super::contracts::FileReviewJob;
use super::super::super::file_context::assemble_file_context;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;
use super::super::jobs::build_file_review_jobs;
use super::super::progress::PreparationProgress;
use super::analysis::PreparedDiffAnalysis;

pub(super) async fn prepare_diff_review_jobs(
    services: &PipelineServices,
    session: &mut ReviewSession,
    progress: &PreparationProgress,
    diff_index: usize,
    diff: &core::UnifiedDiff,
    analysis: PreparedDiffAnalysis,
    next_job_order: usize,
) -> Result<Vec<FileReviewJob>> {
    progress.report_current_file(session, diff);

    let prepared_file = assemble_file_context(
        services,
        session,
        diff,
        analysis.context_chunks,
        analysis.deterministic_comments,
    )
    .await?;

    session
        .verification_context
        .insert(diff.file_path.clone(), prepared_file.context_chunks.clone());
    session
        .graph_query_traces
        .extend(prepared_file.graph_query_traces.clone());

    build_file_review_jobs(
        services,
        session,
        diff_index,
        diff,
        &prepared_file,
        next_job_order,
    )
}
