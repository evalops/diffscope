use crate::adapters;
use crate::core;
use crate::core::offline::optimize_prompt_for_local;
use anyhow::Result;

use super::comments::{filter_comments_for_diff, synthesize_analyzer_comments};
use super::context::{extract_symbols_from_diff, gather_related_file_context};
use super::contracts::{FileReviewJob, PreparedReviewJobs};
use super::guidance::build_review_guidance;
use super::session::{PipelineServices, ReviewSession};
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
        session
            .verification_context
            .insert(diff.file_path.clone(), context_chunks.clone());

        let specialized_passes: Vec<core::SpecializedPassKind> =
            if services.config.multi_pass_specialized {
                let mut passes = vec![
                    core::SpecializedPassKind::Security,
                    core::SpecializedPassKind::Correctness,
                ];
                if services.config.strictness >= 2 {
                    passes.push(core::SpecializedPassKind::Style);
                }
                passes
            } else {
                Vec::new()
            };

        if specialized_passes.is_empty() {
            let mut local_prompt_config = services.base_prompt_config.clone();
            if let Some(custom_prompt) = &services.config.system_prompt {
                local_prompt_config.system_prompt = custom_prompt.clone();
            }
            if let Some(ref path_config) = path_config {
                if let Some(ref prompt) = path_config.system_prompt {
                    local_prompt_config.system_prompt = prompt.clone();
                }
            }
            if let Some(guidance) = build_review_guidance(&services.config, path_config.as_ref()) {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config.system_prompt.push_str(&guidance);
            }
            if !session.enhanced_guidance.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(&session.enhanced_guidance);
            }
            if !services.feedback_context.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(&services.feedback_context);
            }
            if let Some(instructions) = session.auto_instructions.as_ref() {
                local_prompt_config
                    .system_prompt
                    .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                local_prompt_config.system_prompt.push_str(instructions);
            }
            let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
            let (system_prompt, user_prompt) =
                local_prompt_builder.build_prompt(diff, &context_chunks)?;

            let (system_prompt, user_prompt) = if services.is_local {
                let context_window = services.config.context_window.unwrap_or(8192);
                optimize_prompt_for_local(&system_prompt, &user_prompt, context_window)
            } else {
                (system_prompt, user_prompt)
            };

            jobs.push(FileReviewJob {
                job_order: next_job_order,
                diff_index,
                request: adapters::llm::LLMRequest {
                    system_prompt,
                    user_prompt,
                    temperature: None,
                    max_tokens: None,
                    response_schema: Some(review_comments_response_schema()),
                },
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
                let mut local_prompt_config = services.base_prompt_config.clone();
                local_prompt_config.system_prompt = pass_kind.system_prompt();

                if !session.enhanced_guidance.is_empty() {
                    local_prompt_config.system_prompt.push_str("\n\n");
                    local_prompt_config
                        .system_prompt
                        .push_str(&session.enhanced_guidance);
                }
                if let Some(instructions) = session.auto_instructions.as_ref() {
                    local_prompt_config
                        .system_prompt
                        .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                    local_prompt_config.system_prompt.push_str(instructions);
                }

                let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
                let (system_prompt, user_prompt) =
                    local_prompt_builder.build_prompt(diff, &context_chunks)?;

                let (system_prompt, user_prompt) = if services.is_local {
                    let context_window = services.config.context_window.unwrap_or(8192);
                    optimize_prompt_for_local(&system_prompt, &user_prompt, context_window)
                } else {
                    (system_prompt, user_prompt)
                };

                jobs.push(FileReviewJob {
                    job_order: next_job_order,
                    diff_index,
                    request: adapters::llm::LLMRequest {
                        system_prompt,
                        user_prompt,
                        temperature: None,
                        max_tokens: None,
                        response_schema: Some(review_comments_response_schema()),
                    },
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
        files_completed,
        files_skipped,
    })
}

fn review_comments_response_schema() -> adapters::llm::StructuredOutputSchema {
    adapters::llm::StructuredOutputSchema::json_schema(
        "review_findings",
        serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["line", "content", "severity", "category", "confidence", "fix_effort", "tags"],
                "properties": {
                    "line": {"type": "integer", "minimum": 1},
                    "content": {"type": "string"},
                    "severity": {"type": "string", "enum": ["error", "warning", "info", "suggestion"]},
                    "category": {"type": "string", "enum": ["bug", "security", "performance", "style", "best_practice"]},
                    "confidence": {"type": ["number", "string"]},
                    "fix_effort": {"type": "string", "enum": ["low", "medium", "high"]},
                    "rule_id": {"type": ["string", "null"]},
                    "suggestion": {"type": ["string", "null"]},
                    "code_suggestion": {"type": ["string", "null"]},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                }
            }
        }),
    )
}
