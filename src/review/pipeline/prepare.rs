use anyhow::Result;

use crate::core;

use super::comments::{filter_comments_for_diff, synthesize_analyzer_comments};
use super::contracts::{FileReviewJob, PreparedReviewJobs};
use super::file_context::{assemble_file_context, PreparedFileContext};
use super::request::{build_review_request, specialized_passes};
use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::ProgressUpdate;

pub(super) async fn prepare_file_review_jobs(
    services: &PipelineServices,
    session: &mut ReviewSession,
) -> Result<PreparedReviewJobs> {
    let mut all_comments = Vec::new();
    let mut files_completed = 0usize;
    let mut files_skipped = 0usize;
    let mut jobs = Vec::new();
    let mut next_job_order = 0usize;
    let repo_path_str = services.repo_path_str();

    let mut batched_pre_analysis = services
        .plugin_manager
        .run_pre_analyzers_for_review(&session.diffs, &repo_path_str)
        .await?;

    for (diff_index, diff) in session.diffs.iter().enumerate() {
        if services.config.should_exclude(&diff.file_path) {
            tracing::info!("Skipping excluded file: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }
        if diff.is_deleted {
            tracing::info!("Skipping deleted file: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }
        if diff.is_binary || diff.hunks.is_empty() {
            tracing::info!("Skipping non-text diff: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }

        let pre_analysis = batched_pre_analysis
            .remove(&diff.file_path)
            .unwrap_or_default();
        let deterministic_comments = filter_comments_for_diff(
            diff,
            synthesize_analyzer_comments(pre_analysis.findings.clone())?,
        );

        let triage_result = super::super::triage::triage_diff(diff);
        if triage_result.should_skip() {
            if deterministic_comments.is_empty() {
                tracing::info!(
                    "Skipping {} (triage: {})",
                    diff.file_path.display(),
                    triage_result.reason()
                );
                files_skipped += 1;
            } else {
                tracing::info!(
                    "Skipping expensive LLM review for {} (triage: {}), keeping {} analyzer finding(s)",
                    diff.file_path.display(),
                    triage_result.reason(),
                    deterministic_comments.len()
                );
                all_comments.extend(deterministic_comments);
                files_completed += 1;
                if let Some(ref callback) = session.on_progress {
                    callback(ProgressUpdate {
                        current_file: diff.file_path.display().to_string(),
                        files_total: session.files_total,
                        files_completed,
                        files_skipped,
                        comments_so_far: all_comments.clone(),
                    });
                }
            }
            continue;
        }

        if let Some(ref callback) = session.on_progress {
            callback(ProgressUpdate {
                current_file: diff.file_path.display().to_string(),
                files_total: session.files_total,
                files_completed,
                files_skipped,
                comments_so_far: all_comments.clone(),
            });
        }

        let prepared_file = assemble_file_context(
            services,
            session,
            diff,
            pre_analysis.context_chunks,
            deterministic_comments,
        )
        .await?;

        session
            .verification_context
            .insert(diff.file_path.clone(), prepared_file.context_chunks.clone());

        let file_jobs = build_file_review_jobs(
            services,
            session,
            diff_index,
            diff,
            &prepared_file,
            next_job_order,
        )?;
        next_job_order += file_jobs.len();
        jobs.extend(file_jobs);
    }

    Ok(PreparedReviewJobs {
        jobs,
        all_comments,
        files_completed,
        files_skipped,
    })
}

fn build_file_review_jobs(
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
