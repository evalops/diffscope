use anyhow::Result;

use crate::config;
use crate::core;

use super::comments::{filter_comments_for_diff, synthesize_analyzer_comments};
use super::context::{extract_symbols_from_diff, gather_related_file_context};
use super::contracts::{FileReviewJob, PreparedReviewJobs};
use super::request::{build_review_request, specialized_passes};
use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::ProgressUpdate;

struct PreparedFileContext {
    active_rules: Vec<core::ReviewRule>,
    path_config: Option<config::PathConfig>,
    deterministic_comments: Vec<core::Comment>,
    context_chunks: Vec<core::LLMContextChunk>,
}

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
            pre_analysis,
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

async fn assemble_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    pre_analysis: crate::plugins::PreAnalysis,
    deterministic_comments: Vec<core::Comment>,
) -> Result<PreparedFileContext> {
    let mut context_chunks = services
        .context_fetcher
        .fetch_context_for_file(
            &diff.file_path,
            &diff
                .hunks
                .iter()
                .map(|h| (h.new_start, h.new_start + h.new_lines.saturating_sub(1)))
                .collect::<Vec<_>>(),
        )
        .await?;

    context_chunks.extend(pre_analysis.context_chunks.clone());

    let symbols = extract_symbols_from_diff(diff);
    if !symbols.is_empty() {
        let definition_chunks = services
            .context_fetcher
            .fetch_related_definitions(&diff.file_path, &symbols)
            .await?;
        context_chunks.extend(definition_chunks);
        if let Some(index) = session.symbol_index.as_ref() {
            let index_chunks = services
                .context_fetcher
                .fetch_related_definitions_with_index(
                    &diff.file_path,
                    &symbols,
                    index,
                    services.config.symbol_index_max_locations,
                    services.config.symbol_index_graph_hops,
                    services.config.symbol_index_graph_max_files,
                )
                .await?;
            context_chunks.extend(index_chunks);
        }
    }

    if let Some(index) = session.symbol_index.as_ref() {
        let caller_chunks =
            gather_related_file_context(index, &diff.file_path, &services.repo_path);
        context_chunks.extend(caller_chunks);
    }

    if let Some(index) = session.semantic_index.as_ref() {
        let semantic_chunks = core::semantic_context_for_diff(
            index,
            diff,
            session
                .source_files
                .get(&diff.file_path)
                .map(|content| content.as_str()),
            services.embedding_adapter.as_deref(),
            services.config.semantic_rag_top_k,
            services.config.semantic_rag_min_similarity,
        )
        .await;
        context_chunks.extend(semantic_chunks);
    }

    let path_config = services.config.get_path_config(&diff.file_path).cloned();

    if let Some(ref path_config) = path_config {
        if !path_config.focus.is_empty() {
            let focus_chunk = core::LLMContextChunk::documentation(
                diff.file_path.clone(),
                format!(
                    "Focus areas for this file: {}",
                    path_config.focus.join(", ")
                ),
            )
            .with_provenance(core::ContextProvenance::PathSpecificFocusAreas);
            context_chunks.push(focus_chunk);
        }
        if !path_config.extra_context.is_empty() {
            let extra_chunks = services
                .context_fetcher
                .fetch_additional_context(&path_config.extra_context)
                .await?;
            context_chunks.extend(extra_chunks);
        }
    }

    super::super::context_helpers::inject_custom_context(
        &services.config,
        &services.context_fetcher,
        diff,
        &mut context_chunks,
    )
    .await?;
    super::super::context_helpers::inject_pattern_repository_context(
        &services.config,
        &services.pattern_repositories,
        &services.context_fetcher,
        diff,
        &mut context_chunks,
    )
    .await?;

    let active_rules = core::active_rules_for_file(
        &services.review_rules,
        &diff.file_path,
        services.config.max_active_rules,
    );
    super::super::rule_helpers::inject_rule_context(diff, &active_rules, &mut context_chunks);
    context_chunks = super::super::context_helpers::rank_and_trim_context_chunks(
        diff,
        context_chunks,
        services.config.context_max_chunks,
        services.config.context_budget_chars,
    );

    Ok(PreparedFileContext {
        active_rules,
        path_config,
        deterministic_comments,
        context_chunks,
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
