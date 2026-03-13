use anyhow::Result;

use crate::core;
use crate::review::triage::triage_diff;

use super::super::comments::{filter_comments_for_diff, synthesize_analyzer_comments};
use super::super::contracts::PreparedReviewJobs;
use super::super::file_context::assemble_file_context;
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;
use super::jobs::build_file_review_jobs;
use super::progress::PreparationProgress;

pub(in super::super) async fn prepare_file_review_jobs(
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

    for (diff_index, diff) in session.diffs.iter().enumerate() {
        if skip_diff_if_needed(services, diff, &mut progress) {
            continue;
        }

        let pre_analysis = batched_pre_analysis
            .remove(&diff.file_path)
            .unwrap_or_default();
        let deterministic_comments = filter_comments_for_diff(
            diff,
            synthesize_analyzer_comments(pre_analysis.findings.clone())?,
        );

        let triage_result = triage_diff(diff);
        if triage_result.should_skip() {
            if deterministic_comments.is_empty() {
                tracing::info!(
                    "Skipping {} (triage: {})",
                    diff.file_path.display(),
                    triage_result.reason()
                );
                progress.skip_file();
            } else {
                tracing::info!(
                    "Skipping expensive LLM review for {} (triage: {}), keeping {} analyzer finding(s)",
                    diff.file_path.display(),
                    triage_result.reason(),
                    deterministic_comments.len()
                );
                progress.complete_with_comments(session, diff, deterministic_comments);
            }
            continue;
        }

        progress.report_current_file(session, diff);

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

    Ok(progress.into_prepared_review_jobs(jobs))
}

fn skip_diff_if_needed(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    progress: &mut PreparationProgress,
) -> bool {
    let skip_message = if services.config.should_exclude(&diff.file_path) {
        Some("Skipping excluded file")
    } else if diff.is_deleted {
        Some("Skipping deleted file")
    } else if diff.is_binary || diff.hunks.is_empty() {
        Some("Skipping non-text diff")
    } else {
        None
    };

    let Some(skip_message) = skip_message else {
        return false;
    };

    tracing::info!("{}: {}", skip_message, diff.file_path.display());
    progress.skip_file();
    true
}
