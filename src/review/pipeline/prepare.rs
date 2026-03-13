use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::adapters;
use crate::config;
use crate::core;
use crate::core::offline::optimize_prompt_for_local;
use crate::plugins;

use super::execution::FileReviewJob;
use super::helpers::{
    build_review_guidance, extract_symbols_from_diff, filter_comments_for_diff,
    gather_related_file_context, review_comments_response_schema, synthesize_analyzer_comments,
};
use super::{ProgressCallback, ProgressUpdate};

pub(super) struct ReviewPreparationContext<'a> {
    pub diffs: &'a [core::UnifiedDiff],
    pub config: &'a config::Config,
    pub repo_path: &'a Path,
    pub on_progress: Option<ProgressCallback>,
    pub source_files: &'a HashMap<PathBuf, String>,
    pub context_fetcher: &'a core::ContextFetcher,
    pub symbol_index: &'a Option<core::SymbolIndex>,
    pub semantic_index: Option<&'a core::semantic::SemanticIndex>,
    pub embedding_adapter: Option<&'a dyn adapters::llm::LLMAdapter>,
    pub pattern_repositories: &'a super::super::context_helpers::PatternRepositoryMap,
    pub review_rules: &'a [core::ReviewRule],
    pub feedback_context: &'a str,
    pub base_prompt_config: &'a core::prompt::PromptConfig,
    pub enhanced_guidance: &'a str,
    pub auto_instructions: Option<&'a String>,
    pub batched_pre_analysis: HashMap<PathBuf, plugins::PreAnalysis>,
    pub is_local: bool,
}

pub(super) struct PreparedReviewJobs {
    pub jobs: Vec<FileReviewJob>,
    pub all_comments: Vec<core::Comment>,
    pub verification_context: HashMap<PathBuf, Vec<core::LLMContextChunk>>,
    pub files_completed: usize,
    pub files_skipped: usize,
}

pub(super) async fn prepare_file_review_jobs(
    mut context: ReviewPreparationContext<'_>,
) -> Result<PreparedReviewJobs> {
    let mut all_comments = Vec::new();
    let mut verification_context: HashMap<PathBuf, Vec<core::LLMContextChunk>> = HashMap::new();
    let mut files_completed = 0usize;
    let mut files_skipped = 0usize;
    let files_total = context.diffs.len();
    let mut jobs = Vec::new();
    let mut next_job_order = 0usize;

    for (diff_index, diff) in context.diffs.iter().enumerate() {
        if context.config.should_exclude(&diff.file_path) {
            info!("Skipping excluded file: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }
        if diff.is_deleted {
            info!("Skipping deleted file: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }
        if diff.is_binary || diff.hunks.is_empty() {
            info!("Skipping non-text diff: {}", diff.file_path.display());
            files_skipped += 1;
            continue;
        }

        let pre_analysis = context
            .batched_pre_analysis
            .remove(&diff.file_path)
            .unwrap_or_default();
        let deterministic_comments = filter_comments_for_diff(
            diff,
            synthesize_analyzer_comments(pre_analysis.findings.clone())?,
        );

        let triage_result = super::super::triage::triage_diff(diff);
        if triage_result.should_skip() {
            if deterministic_comments.is_empty() {
                info!(
                    "Skipping {} (triage: {})",
                    diff.file_path.display(),
                    triage_result.reason()
                );
                files_skipped += 1;
            } else {
                info!(
                    "Skipping expensive LLM review for {} (triage: {}), keeping {} analyzer finding(s)",
                    diff.file_path.display(),
                    triage_result.reason(),
                    deterministic_comments.len()
                );
                all_comments.extend(deterministic_comments);
                files_completed += 1;
                if let Some(ref cb) = context.on_progress {
                    cb(ProgressUpdate {
                        current_file: diff.file_path.display().to_string(),
                        files_total,
                        files_completed,
                        files_skipped,
                        comments_so_far: all_comments.clone(),
                    });
                }
            }
            continue;
        }

        if let Some(ref cb) = context.on_progress {
            cb(ProgressUpdate {
                current_file: diff.file_path.display().to_string(),
                files_total,
                files_completed,
                files_skipped,
                comments_so_far: all_comments.clone(),
            });
        }

        let mut context_chunks = context
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
            let definition_chunks = context
                .context_fetcher
                .fetch_related_definitions(&diff.file_path, &symbols)
                .await?;
            context_chunks.extend(definition_chunks);
            if let Some(index) = context.symbol_index {
                let index_chunks = context
                    .context_fetcher
                    .fetch_related_definitions_with_index(
                        &diff.file_path,
                        &symbols,
                        index,
                        context.config.symbol_index_max_locations,
                        context.config.symbol_index_graph_hops,
                        context.config.symbol_index_graph_max_files,
                    )
                    .await?;
                context_chunks.extend(index_chunks);
            }
        }

        if let Some(ref index) = context.symbol_index {
            let caller_chunks =
                gather_related_file_context(index, &diff.file_path, context.repo_path);
            context_chunks.extend(caller_chunks);
        }

        if let Some(index) = context.semantic_index {
            let semantic_chunks = core::semantic_context_for_diff(
                index,
                diff,
                context
                    .source_files
                    .get(&diff.file_path)
                    .map(|content| content.as_str()),
                context.embedding_adapter,
                context.config.semantic_rag_top_k,
                context.config.semantic_rag_min_similarity,
            )
            .await;
            context_chunks.extend(semantic_chunks);
        }

        let path_config = context.config.get_path_config(&diff.file_path).cloned();

        if let Some(ref pc) = path_config {
            if !pc.focus.is_empty() {
                let focus_chunk = core::LLMContextChunk::documentation(
                    diff.file_path.clone(),
                    format!("Focus areas for this file: {}", pc.focus.join(", ")),
                )
                .with_provenance(core::ContextProvenance::PathSpecificFocusAreas);
                context_chunks.push(focus_chunk);
            }
            if !pc.extra_context.is_empty() {
                let extra_chunks = context
                    .context_fetcher
                    .fetch_additional_context(&pc.extra_context)
                    .await?;
                context_chunks.extend(extra_chunks);
            }
        }
        super::super::context_helpers::inject_custom_context(
            context.config,
            context.context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;
        super::super::context_helpers::inject_pattern_repository_context(
            context.config,
            context.pattern_repositories,
            context.context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;
        let active_rules = core::active_rules_for_file(
            context.review_rules,
            &diff.file_path,
            context.config.max_active_rules,
        );
        super::super::rule_helpers::inject_rule_context(diff, &active_rules, &mut context_chunks);
        context_chunks = super::super::context_helpers::rank_and_trim_context_chunks(
            diff,
            context_chunks,
            context.config.context_max_chunks,
            context.config.context_budget_chars,
        );
        verification_context.insert(diff.file_path.clone(), context_chunks.clone());

        let specialized_passes: Vec<core::SpecializedPassKind> =
            if context.config.multi_pass_specialized {
                let mut passes = vec![
                    core::SpecializedPassKind::Security,
                    core::SpecializedPassKind::Correctness,
                ];
                if context.config.strictness >= 2 {
                    passes.push(core::SpecializedPassKind::Style);
                }
                passes
            } else {
                Vec::new()
            };

        if specialized_passes.is_empty() {
            let mut local_prompt_config = context.base_prompt_config.clone();
            if let Some(custom_prompt) = &context.config.system_prompt {
                local_prompt_config.system_prompt = custom_prompt.clone();
            }
            if let Some(ref pc) = path_config {
                if let Some(ref prompt) = pc.system_prompt {
                    local_prompt_config.system_prompt = prompt.clone();
                }
            }
            if let Some(guidance) = build_review_guidance(context.config, path_config.as_ref()) {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config.system_prompt.push_str(&guidance);
            }
            if !context.enhanced_guidance.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(context.enhanced_guidance);
            }
            if !context.feedback_context.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(context.feedback_context);
            }
            if let Some(instructions) = context.auto_instructions {
                local_prompt_config
                    .system_prompt
                    .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                local_prompt_config.system_prompt.push_str(instructions);
            }
            let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
            let (system_prompt, user_prompt) =
                local_prompt_builder.build_prompt(diff, &context_chunks)?;

            let (system_prompt, user_prompt) = if context.is_local {
                let context_window = context.config.context_window.unwrap_or(8192);
                optimize_prompt_for_local(&system_prompt, &user_prompt, context_window)
            } else {
                (system_prompt, user_prompt)
            };

            let request = adapters::llm::LLMRequest {
                system_prompt,
                user_prompt,
                temperature: None,
                max_tokens: None,
                response_schema: Some(review_comments_response_schema()),
            };

            jobs.push(FileReviewJob {
                job_order: next_job_order,
                diff_index,
                request,
                active_rules,
                path_config,
                file_path: diff.file_path.clone(),
                deterministic_comments: deterministic_comments.clone(),
                pass_kind: None,
                mark_file_complete: true,
            });
            next_job_order += 1;
        } else {
            for (pass_index, pass_kind) in specialized_passes.iter().enumerate() {
                let deterministic_comments_for_job =
                    if specialized_passes.first() == Some(pass_kind) {
                        deterministic_comments.clone()
                    } else {
                        Vec::new()
                    };
                let mut local_prompt_config = context.base_prompt_config.clone();
                local_prompt_config.system_prompt = pass_kind.system_prompt();

                if !context.enhanced_guidance.is_empty() {
                    local_prompt_config.system_prompt.push_str("\n\n");
                    local_prompt_config
                        .system_prompt
                        .push_str(context.enhanced_guidance);
                }
                if let Some(instructions) = context.auto_instructions {
                    local_prompt_config
                        .system_prompt
                        .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                    local_prompt_config.system_prompt.push_str(instructions);
                }

                let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
                let (system_prompt, user_prompt) =
                    local_prompt_builder.build_prompt(diff, &context_chunks)?;

                let (system_prompt, user_prompt) = if context.is_local {
                    let context_window = context.config.context_window.unwrap_or(8192);
                    optimize_prompt_for_local(&system_prompt, &user_prompt, context_window)
                } else {
                    (system_prompt, user_prompt)
                };

                let request = adapters::llm::LLMRequest {
                    system_prompt,
                    user_prompt,
                    temperature: None,
                    max_tokens: None,
                    response_schema: Some(review_comments_response_schema()),
                };

                jobs.push(FileReviewJob {
                    job_order: next_job_order,
                    diff_index,
                    request,
                    active_rules: active_rules.clone(),
                    path_config: path_config.clone(),
                    file_path: diff.file_path.clone(),
                    deterministic_comments: deterministic_comments_for_job,
                    pass_kind: Some(*pass_kind),
                    mark_file_complete: pass_index + 1 == specialized_passes.len(),
                });
                next_job_order += 1;
            }
        }
    }

    Ok(PreparedReviewJobs {
        jobs,
        all_comments,
        verification_context,
        files_completed,
        files_skipped,
    })
}
