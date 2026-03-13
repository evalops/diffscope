use anyhow::Result;

use crate::core;

use super::super::contracts::FileReviewJob;
use super::super::file_context::PreparedFileContext;
use super::super::request::{build_review_request, specialized_passes};
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;

pub(super) fn build_file_review_jobs(
    services: &PipelineServices,
    session: &ReviewSession,
    diff_index: usize,
    diff: &core::UnifiedDiff,
    prepared_file: &PreparedFileContext,
    next_job_order: usize,
) -> Result<Vec<FileReviewJob>> {
    let pass_kinds = specialized_passes(&services.config);
    let mut jobs = Vec::new();

    if pass_kinds.is_empty() {
        jobs.push(FileReviewJob {
            job_order: next_job_order,
            diff_index,
            request: build_review_request(
                services,
                session,
                diff,
                &prepared_file.context_chunks,
                prepared_file.path_config.as_ref(),
                None,
            )?,
            active_rules: prepared_file.active_rules.clone(),
            path_config: prepared_file.path_config.clone(),
            file_path: diff.file_path.clone(),
            deterministic_comments: prepared_file.deterministic_comments.clone(),
            pass_kind: None,
            mark_file_complete: true,
        });
        return Ok(jobs);
    }

    let total_passes = pass_kinds.len();
    for (pass_index, pass_kind) in pass_kinds.into_iter().enumerate() {
        let deterministic_comments = if pass_index == 0 {
            prepared_file.deterministic_comments.clone()
        } else {
            Vec::new()
        };

        jobs.push(FileReviewJob {
            job_order: next_job_order + pass_index,
            diff_index,
            request: build_review_request(
                services,
                session,
                diff,
                &prepared_file.context_chunks,
                prepared_file.path_config.as_ref(),
                Some(pass_kind),
            )?,
            active_rules: prepared_file.active_rules.clone(),
            path_config: prepared_file.path_config.clone(),
            file_path: diff.file_path.clone(),
            deterministic_comments,
            pass_kind: Some(pass_kind),
            mark_file_complete: pass_index + 1 == total_passes,
        });
    }

    Ok(jobs)
}
