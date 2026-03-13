use anyhow::Result;

use super::super::super::contracts::PreparedReviewJobs;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;
use super::super::progress::PreparationProgress;
use super::analysis::{prepare_diff_analysis, DiffPreparationDecision};
use super::diff::prepare_diff_review_jobs;
use super::skip::skip_diff_if_needed;

pub(in super::super::super) async fn prepare_file_review_jobs(
    services: &PipelineServices,
    session: &mut ReviewSession,
) -> Result<PreparedReviewJobs> {
    let mut progress = PreparationProgress::new();
    let mut jobs = Vec::new();
    let mut next_job_order = 0usize;
    let repo_path_str = services.repo_path_str();

    let mut batched_pre_analysis = services
        .plugin_manager
        .run_pre_analyzers_for_review(&session.diffs, &repo_path_str)
        .await?;

    for diff_index in 0..session.diffs.len() {
        let diff = session.diffs[diff_index].clone();

        if skip_diff_if_needed(services, &diff, &mut progress) {
            continue;
        }

        match prepare_diff_analysis(&diff, &mut batched_pre_analysis)? {
            DiffPreparationDecision::Skip => {
                progress.skip_file();
            }
            DiffPreparationDecision::CompleteWithComments(comments) => {
                progress.complete_with_comments(session, &diff, comments);
            }
            DiffPreparationDecision::Review(analysis) => {
                let file_jobs = prepare_diff_review_jobs(
                    services,
                    session,
                    &progress,
                    diff_index,
                    &diff,
                    analysis,
                    next_job_order,
                )
                .await?;
                next_job_order += file_jobs.len();
                jobs.extend(file_jobs);
            }
        }
    }

    Ok(progress.into_prepared_review_jobs(jobs))
}
