use anyhow::Result;
use futures::StreamExt;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, warn};

use std::sync::Arc;

use crate::adapters;
use crate::config;
use crate::core;
use crate::core::convention_learner::ConventionStore;
use crate::core::offline::optimize_prompt_for_local;
use crate::output::OutputFormat;
use crate::parsing::parse_llm_response;
use crate::plugins;

use super::triage;

/// Agent activity metadata from the agent loop.
#[derive(Debug, Clone)]
pub struct AgentActivity {
    pub total_iterations: usize,
    pub tool_calls: Vec<core::agent_loop::AgentToolCallLog>,
}

/// Rich result from the review pipeline, carrying comments plus telemetry metadata.
#[derive(Debug, Clone, Default)]
pub struct ReviewResult {
    pub comments: Vec<core::Comment>,
    /// Aggregate LLM token usage across all files and passes.
    pub total_prompt_tokens: usize,
    pub total_completion_tokens: usize,
    pub total_tokens: usize,
    /// Per-file metrics (latency, tokens, comment count).
    pub file_metrics: Vec<FileMetric>,
    /// Number of comments suppressed by learned convention patterns.
    pub convention_suppressed_count: usize,
    /// Comment counts grouped by specialized pass tag (e.g., "security-pass": 3).
    pub comments_by_pass: HashMap<String, usize>,
    /// Hotspot detection results from multi-pass analysis.
    pub hotspots: Vec<core::multi_pass::HotspotResult>,
    /// Agent loop activity (None for one-shot reviews).
    pub agent_activity: Option<AgentActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetric {
    pub file_path: PathBuf,
    pub latency_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub comment_count: usize,
}

/// Progress update emitted per file during review.
pub struct ProgressUpdate {
    pub current_file: String,
    pub files_total: usize,
    pub files_completed: usize,
    pub files_skipped: usize,
    /// Comments found so far (accumulated from all completed files).
    pub comments_so_far: Vec<core::Comment>,
}

/// Callback invoked before each file's LLM call and after completion.
pub type ProgressCallback = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;
use super::context_helpers::{
    inject_custom_context, inject_pattern_repository_context, rank_and_trim_context_chunks,
    resolve_pattern_repositories,
};
use super::feedback::{derive_file_patterns, load_feedback_store};
use super::filters::apply_review_filters;
use super::rule_helpers::{apply_rule_overrides, inject_rule_context, load_review_rules};

pub async fn review_diff_content(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    review_diff_content_with_repo(diff_content, config, format, Path::new(".")).await
}

pub async fn review_diff_content_with_repo(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
    repo_path: &Path,
) -> Result<()> {
    let rule_priority = config.rule_priority.clone();
    let result = review_diff_content_raw(diff_content, config, repo_path).await?;
    crate::output::output_comments(&result.comments, None, format, &rule_priority).await
}

pub async fn review_diff_content_raw(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
) -> Result<ReviewResult> {
    review_diff_content_raw_with_progress(diff_content, config, repo_path, None).await
}

/// Like `review_diff_content_raw` but with an optional progress callback.
#[tracing::instrument(name = "review_pipeline", skip(diff_content, config, repo_path, on_progress), fields(diff_bytes = diff_content.len(), model = %config.model))]
pub async fn review_diff_content_raw_with_progress(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    // For local models, chunk oversized diffs instead of truncating
    if should_optimize_for_local(&config) {
        let context_budget = config.context_window.unwrap_or(8192);
        // Reserve ~40% of context window for the diff (rest for system prompt, context, response)
        let max_diff_chars = (context_budget * 2 / 5).max(1000);
        let chunks = chunk_diff_for_context(diff_content, max_diff_chars);
        if chunks.len() > 1 {
            eprintln!(
                "Diff split into {} chunks for local model context window",
                chunks.len()
            );
            let mut merged = ReviewResult::default();
            for (i, chunk) in chunks.iter().enumerate() {
                eprintln!("Processing chunk {}/{}...", i + 1, chunks.len());
                match review_diff_content_raw_inner(
                    chunk,
                    config.clone(),
                    repo_path,
                    on_progress.clone(),
                )
                .await
                {
                    Ok(chunk_result) => {
                        merged.comments.extend(chunk_result.comments);
                        merged.total_prompt_tokens += chunk_result.total_prompt_tokens;
                        merged.total_completion_tokens += chunk_result.total_completion_tokens;
                        merged.total_tokens += chunk_result.total_tokens;
                        merged.file_metrics.extend(chunk_result.file_metrics);
                        merged.convention_suppressed_count +=
                            chunk_result.convention_suppressed_count;
                        for (pass, count) in chunk_result.comments_by_pass {
                            *merged.comments_by_pass.entry(pass).or_insert(0) += count;
                        }
                        merged.hotspots.extend(chunk_result.hotspots);
                    }
                    Err(e) => {
                        eprintln!("Warning: chunk {} failed: {}", i + 1, e);
                    }
                }
            }
            return Ok(merged);
        }
    }

    review_diff_content_raw_inner(diff_content, config, repo_path, on_progress).await
}

async fn review_diff_content_raw_inner(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewResult> {
    let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
    info!("Parsed {} file diffs", diffs.len());

    let source_files: HashMap<PathBuf, String> = diffs
        .iter()
        .filter_map(|diff| {
            std::fs::read_to_string(repo_path.join(&diff.file_path))
                .ok()
                .map(|content| (diff.file_path.clone(), content))
        })
        .collect();

    // Pre-count reviewable files for progress tracking
    let files_total = diffs.len();
    let mut files_completed: usize = 0;
    let mut files_skipped: usize = 0;

    // Check file change limit
    if let Some(limit) = config.file_change_limit {
        if limit > 0 && diffs.len() > limit {
            anyhow::bail!(
                "Diff contains {} files, exceeding file_change_limit of {}. \
                 Increase the limit or split the review.",
                diffs.len(),
                limit
            );
        }
    }

    // Gather git history for enhanced context
    let git_log_output = gather_git_log(repo_path);

    // Load convention store for learned review patterns
    let convention_store_path = resolve_convention_store_path(&config);
    let convention_json = convention_store_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    // Build enhanced review context with real data from the repository
    let mut enhanced_ctx = core::build_enhanced_context(
        &diffs,
        &source_files,
        git_log_output.as_deref(),
        None,
        convention_json.as_deref(),
        None,
    );
    let enhanced_guidance = core::generate_enhanced_guidance(&enhanced_ctx, "rs");
    if !enhanced_guidance.is_empty() {
        info!(
            "Enhanced guidance generated ({} chars)",
            enhanced_guidance.len()
        );
    }

    // Auto-detect instruction files (.cursorrules, CLAUDE.md, agents.md, etc.)
    let auto_instructions =
        if config.auto_detect_instructions && config.review_instructions.is_none() {
            let detected = detect_instruction_files(repo_path);
            if !detected.is_empty() {
                let combined: Vec<String> = detected
                    .iter()
                    .map(|(name, content)| format!("# From {}\n{}", name, content))
                    .collect();
                Some(combined.join("\n\n"))
            } else {
                None
            }
        } else {
            None
        };

    let symbol_index = build_symbol_index(&config, repo_path);
    let pattern_repositories = resolve_pattern_repositories(&config, repo_path);
    let review_rules = load_review_rules(&config, &pattern_repositories, repo_path);

    // Initialize plugin manager and load builtin plugins
    let mut plugin_manager = plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;
    let feedback = load_feedback_store(&config);
    let feedback_context = if config.enhanced_feedback {
        super::feedback::generate_feedback_context(&feedback)
    } else {
        String::new()
    };

    let model_config = config.to_model_config();

    let adapter: Arc<dyn adapters::llm::LLMAdapter> =
        Arc::from(adapters::llm::create_adapter(&model_config)?);
    info!("Review adapter: {}", adapter.model_name());

    // Use configured model role for verification pass (default: Weak)
    let verification_adapter: Arc<dyn adapters::llm::LLMAdapter> = {
        let verification_config = config.to_model_config_for_role(config.verification_model_role);
        if verification_config.model_name != model_config.model_name {
            info!(
                "Using '{}' model '{}' for verification pass",
                format!("{:?}", config.verification_model_role).to_lowercase(),
                verification_config.model_name
            );
            Arc::from(adapters::llm::create_adapter(&verification_config)?)
        } else {
            adapter.clone()
        }
    };

    let embedding_adapter: Option<Arc<dyn adapters::llm::LLMAdapter>> =
        if config.semantic_rag || config.semantic_feedback {
            let embedding_config = config.to_model_config_for_role(config::ModelRole::Embedding);
            if embedding_config.model_name == model_config.model_name {
                Some(adapter.clone())
            } else {
                match adapters::llm::create_adapter(&embedding_config) {
                    Ok(adapter) => Some(Arc::from(adapter)),
                    Err(e) => {
                        warn!(
                            "Embedding adapter initialization failed for '{}': {}",
                            embedding_config.model_name, e
                        );
                        None
                    }
                }
            }
        } else {
            None
        };

    let semantic_index = if config.semantic_rag {
        let index_path = core::default_index_path(repo_path);
        let changed_files = diffs
            .iter()
            .map(|diff| diff.file_path.clone())
            .collect::<Vec<_>>();
        match core::refresh_semantic_index(
            repo_path,
            &index_path,
            embedding_adapter.as_deref(),
            &changed_files,
            |path| config.should_exclude(path),
            config.semantic_rag_max_files,
        )
        .await
        {
            Ok(index) => Some(index),
            Err(e) => {
                warn!("Semantic index refresh failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    let semantic_feedback_store = if config.semantic_feedback {
        let path = core::default_semantic_feedback_path(&config.feedback_path);
        let mut store = core::load_semantic_feedback_store(&path);
        core::align_semantic_feedback_store(&mut store, embedding_adapter.as_deref());
        Some(store)
    } else {
        None
    };

    let base_prompt_config = core::prompt::PromptConfig {
        max_context_chars: config.max_context_chars,
        max_diff_chars: config.max_diff_chars,
        ..Default::default()
    };
    let mut all_comments = Vec::new();

    let repo_path_str = repo_path.to_string_lossy().to_string();
    let context_fetcher = core::ContextFetcher::new(repo_path.to_path_buf());
    let is_local = should_optimize_for_local(&config);
    let mut batched_pre_analysis = plugin_manager
        .run_pre_analyzers_for_review(&diffs, &repo_path_str)
        .await?;

    // Phase 1: Prepare LLM requests for each file (sequential context gathering)
    struct FileReviewJob {
        job_order: usize,
        diff_index: usize,
        request: adapters::llm::LLMRequest,
        active_rules: Vec<crate::core::ReviewRule>,
        path_config: Option<config::PathConfig>,
        file_path: std::path::PathBuf,
        deterministic_comments: Vec<core::Comment>,
        /// When running specialized multi-pass review, identifies which pass this job belongs to.
        pass_kind: Option<core::SpecializedPassKind>,
        mark_file_complete: bool,
    }

    let mut jobs: Vec<FileReviewJob> = Vec::new();
    let mut next_job_order = 0usize;

    for (diff_index, diff) in diffs.iter().enumerate() {
        // Check if file should be excluded
        if config.should_exclude(&diff.file_path) {
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

        let pre_analysis = batched_pre_analysis
            .remove(&diff.file_path)
            .unwrap_or_default();
        let deterministic_comments = filter_comments_for_diff(
            diff,
            synthesize_analyzer_comments(pre_analysis.findings.clone())?,
        );

        // Triage: skip files that don't need expensive LLM review
        let triage_result = triage::triage_diff(diff);
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
                if let Some(ref cb) = on_progress {
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

        // Emit progress: preparing this file
        if let Some(ref cb) = on_progress {
            cb(ProgressUpdate {
                current_file: diff.file_path.display().to_string(),
                files_total,
                files_completed,
                files_skipped,
                comments_so_far: all_comments.clone(),
            });
        }

        let mut context_chunks = context_fetcher
            .fetch_context_for_file(
                &diff.file_path,
                &diff
                    .hunks
                    .iter()
                    .map(|h| (h.new_start, h.new_start + h.new_lines.saturating_sub(1)))
                    .collect::<Vec<_>>(),
            )
            .await?;

        // Run deterministic pre-analyzers before prompting the LLM.
        context_chunks.extend(pre_analysis.context_chunks.clone());

        // Extract symbols from diff and fetch their definitions
        let symbols = extract_symbols_from_diff(diff);
        if !symbols.is_empty() {
            let definition_chunks = context_fetcher
                .fetch_related_definitions(&diff.file_path, &symbols)
                .await?;
            context_chunks.extend(definition_chunks);
            if let Some(index) = &symbol_index {
                let index_chunks = context_fetcher
                    .fetch_related_definitions_with_index(
                        &diff.file_path,
                        &symbols,
                        index,
                        config.symbol_index_max_locations,
                        config.symbol_index_graph_hops,
                        config.symbol_index_graph_max_files,
                    )
                    .await?;
                context_chunks.extend(index_chunks);
            }
        }

        // Include related files: reverse dependencies (callers) and test files
        if let Some(ref index) = symbol_index {
            let caller_chunks = gather_related_file_context(index, &diff.file_path, repo_path);
            context_chunks.extend(caller_chunks);
        }

        if let Some(index) = semantic_index.as_ref() {
            let semantic_chunks = core::semantic_context_for_diff(
                index,
                diff,
                source_files
                    .get(&diff.file_path)
                    .map(|content| content.as_str()),
                embedding_adapter.as_deref(),
                config.semantic_rag_top_k,
                config.semantic_rag_min_similarity,
            )
            .await;
            context_chunks.extend(semantic_chunks);
        }

        // Get path-specific configuration
        let path_config = config.get_path_config(&diff.file_path).cloned();

        // Add focus areas and extra context if configured
        if let Some(ref pc) = path_config {
            if !pc.focus.is_empty() {
                let focus_chunk = core::LLMContextChunk {
                    content: format!("Focus areas for this file: {}", pc.focus.join(", ")),
                    context_type: core::ContextType::Documentation,
                    file_path: diff.file_path.clone(),
                    line_range: None,
                };
                context_chunks.push(focus_chunk);
            }
            if !pc.extra_context.is_empty() {
                let extra_chunks = context_fetcher
                    .fetch_additional_context(&pc.extra_context)
                    .await?;
                context_chunks.extend(extra_chunks);
            }
        }
        inject_custom_context(&config, &context_fetcher, diff, &mut context_chunks).await?;
        inject_pattern_repository_context(
            &config,
            &pattern_repositories,
            &context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;
        let active_rules =
            core::active_rules_for_file(&review_rules, &diff.file_path, config.max_active_rules);
        inject_rule_context(diff, &active_rules, &mut context_chunks);
        context_chunks = rank_and_trim_context_chunks(
            diff,
            context_chunks,
            config.context_max_chunks,
            config.context_budget_chars,
        );

        // Determine which specialized passes to run, if any.
        let specialized_passes: Vec<core::SpecializedPassKind> = if config.multi_pass_specialized {
            let mut passes = vec![
                core::SpecializedPassKind::Security,
                core::SpecializedPassKind::Correctness,
            ];
            // Only run the style pass when strictness >= 2
            if config.strictness >= 2 {
                passes.push(core::SpecializedPassKind::Style);
            }
            passes
        } else {
            Vec::new()
        };

        if specialized_passes.is_empty() {
            // Standard single-pass mode
            let mut local_prompt_config = base_prompt_config.clone();
            if let Some(custom_prompt) = &config.system_prompt {
                local_prompt_config.system_prompt = custom_prompt.clone();
            }
            if let Some(ref pc) = path_config {
                if let Some(ref prompt) = pc.system_prompt {
                    local_prompt_config.system_prompt = prompt.clone();
                }
            }
            if let Some(guidance) = build_review_guidance(&config, path_config.as_ref()) {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config.system_prompt.push_str(&guidance);
            }
            if !enhanced_guidance.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(&enhanced_guidance);
            }
            if !feedback_context.is_empty() {
                local_prompt_config.system_prompt.push_str("\n\n");
                local_prompt_config
                    .system_prompt
                    .push_str(&feedback_context);
            }
            if let Some(ref instructions) = auto_instructions {
                local_prompt_config
                    .system_prompt
                    .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                local_prompt_config.system_prompt.push_str(instructions);
            }
            let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
            let (system_prompt, user_prompt) =
                local_prompt_builder.build_prompt(diff, &context_chunks)?;

            let (system_prompt, user_prompt) = if is_local {
                let context_window = config.context_window.unwrap_or(8192);
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
            // Multi-pass specialized mode: create one job per pass per file
            for (pass_index, pass_kind) in specialized_passes.iter().enumerate() {
                let deterministic_comments_for_job =
                    if specialized_passes.first() == Some(pass_kind) {
                        deterministic_comments.clone()
                    } else {
                        Vec::new()
                    };
                let mut local_prompt_config = base_prompt_config.clone();
                local_prompt_config.system_prompt = pass_kind.system_prompt();

                if !enhanced_guidance.is_empty() {
                    local_prompt_config.system_prompt.push_str("\n\n");
                    local_prompt_config
                        .system_prompt
                        .push_str(&enhanced_guidance);
                }
                if let Some(ref instructions) = auto_instructions {
                    local_prompt_config
                        .system_prompt
                        .push_str("\n\n# Project-specific instructions (auto-detected):\n");
                    local_prompt_config.system_prompt.push_str(instructions);
                }

                let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
                let (system_prompt, user_prompt) =
                    local_prompt_builder.build_prompt(diff, &context_chunks)?;

                let (system_prompt, user_prompt) = if is_local {
                    let context_window = config.context_window.unwrap_or(8192);
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

    // Phase 2: Send LLM requests with bounded concurrency
    const MAX_CONCURRENT_FILES: usize = 5;
    let concurrency = if is_local { 1 } else { MAX_CONCURRENT_FILES };

    info!(
        "Sending {} LLM requests (concurrency={})",
        jobs.len(),
        concurrency,
    );

    // Build agent tools context if agent_review is enabled
    let agent_tool_ctx = if config.agent_review && adapter.supports_tools() {
        let context_fetcher_arc = Arc::new(core::ContextFetcher::new(repo_path.to_path_buf()));
        Some(Arc::new(core::agent_tools::ReviewToolContext {
            repo_path: repo_path.to_path_buf(),
            context_fetcher: context_fetcher_arc,
            symbol_index: None, // Agent can use lookup_symbol/search_codebase tools instead
            symbol_graph: None,
            git_history: None,
        }))
    } else {
        None
    };
    let agent_loop_config = core::agent_loop::AgentLoopConfig {
        max_iterations: config.agent_max_iterations,
        max_total_tokens: config.agent_max_total_tokens,
    };
    let agent_tools_filter = config.agent_tools_enabled.clone();

    let on_progress_ref = &on_progress;
    let files_skipped_snapshot = files_skipped;

    let results: Vec<_> = futures::stream::iter(jobs)
        .map(|job| {
            let adapter = adapter.clone();
            let agent_ctx = agent_tool_ctx.clone();
            let loop_config = agent_loop_config.clone();
            let tools_filter = agent_tools_filter.clone();
            async move {
                if is_local {
                    eprintln!("Sending {} to local model...", job.file_path.display());
                }
                let request_start = Instant::now();

                let (response, agent_data) = if let Some(ctx) = agent_ctx {
                    // Agent mode: iterative tool-calling loop
                    let tools = core::agent_tools::build_review_tools(ctx, tools_filter.as_deref());
                    let tool_defs: Vec<_> = tools.iter().map(|t| t.definition()).collect();
                    let chat_request =
                        adapters::llm::ChatRequest::from_llm_request(job.request, &tool_defs);
                    match core::agent_loop::run_agent_loop(
                        adapter.as_ref(),
                        chat_request,
                        &tools,
                        &loop_config,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            let activity = AgentActivity {
                                total_iterations: result.iterations,
                                tool_calls: result.tool_calls,
                            };
                            (
                                Ok(adapters::llm::LLMResponse {
                                    content: result.content,
                                    model: result.model,
                                    usage: Some(result.total_usage),
                                }),
                                Some(activity),
                            )
                        }
                        Err(e) => (Err(e), None),
                    }
                } else {
                    // Standard one-shot mode
                    (adapter.complete(job.request).await, None)
                };

                let latency_ms = request_start.elapsed().as_millis() as u64;
                if is_local {
                    eprintln!(
                        "{}: response received ({:.1}s)",
                        job.file_path.display(),
                        latency_ms as f64 / 1000.0
                    );
                }
                (
                    job.job_order,
                    job.diff_index,
                    job.active_rules,
                    job.path_config,
                    job.file_path,
                    job.deterministic_comments,
                    job.pass_kind,
                    job.mark_file_complete,
                    response,
                    latency_ms,
                    agent_data,
                )
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // Phase 3: Process results in file order
    let mut indexed_results = results;
    indexed_results.sort_by_key(|(job_order, _, _, _, _, _, _, _, _, _, _)| *job_order);

    let mut total_prompt_tokens: usize = 0;
    let mut total_completion_tokens: usize = 0;
    let mut total_tokens: usize = 0;
    let mut file_metrics: Vec<FileMetric> = Vec::new();
    let mut comments_by_pass: HashMap<String, usize> = HashMap::new();
    let mut aggregate_agent_iterations: usize = 0;
    let mut aggregate_agent_tool_calls: Vec<core::agent_loop::AgentToolCallLog> = Vec::new();
    let mut has_agent_activity = false;

    for (
        _job_order,
        diff_index,
        active_rules,
        path_config,
        file_path,
        deterministic_comments,
        pass_kind,
        mark_file_complete,
        response,
        latency_ms,
        agent_data,
    ) in indexed_results
    {
        let diff = &diffs[diff_index];

        match response {
            Err(e) => {
                warn!("LLM request failed for {}: {}", file_path.display(), e);
                merge_file_metric(
                    &mut file_metrics,
                    &file_path,
                    latency_ms,
                    0,
                    0,
                    0,
                    deterministic_comments.len(),
                );
                all_comments.extend(deterministic_comments);
            }
            Ok(response) => {
                // Extract usage metrics from the response
                let (resp_prompt_tokens, resp_completion_tokens, resp_total_tokens) =
                    if let Some(ref usage) = response.usage {
                        (
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens,
                        )
                    } else {
                        (0, 0, 0)
                    };
                total_prompt_tokens += resp_prompt_tokens;
                total_completion_tokens += resp_completion_tokens;
                total_tokens += resp_total_tokens;

                // Accumulate agent activity if present
                if let Some(activity) = agent_data {
                    has_agent_activity = true;
                    aggregate_agent_iterations += activity.total_iterations;
                    aggregate_agent_tool_calls.extend(activity.tool_calls);
                }

                // Validate LLM response before parsing
                if let Err(validation_err) = validate_llm_response(&response.content) {
                    eprintln!(
                        "Warning: LLM response validation failed for {}: {}",
                        file_path.display(),
                        validation_err
                    );
                    if is_local {
                        eprintln!(
                            "Hint: Try a larger model or reduce diff size for better results with local models."
                        );
                    }
                    // Still record file metric even for validation failures
                    let analyzer_comment_count = deterministic_comments.len();
                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        analyzer_comment_count,
                    );

                    all_comments.extend(deterministic_comments);
                    continue;
                }

                if let Ok(raw_comments) = parse_llm_response(&response.content, &diff.file_path) {
                    let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;

                    // Tag comments with the specialized pass kind, if applicable
                    if let Some(kind) = pass_kind {
                        for comment in &mut comments {
                            if !comment.tags.contains(&kind.tag().to_string()) {
                                comment.tags.push(kind.tag().to_string());
                            }
                        }
                    }

                    // Apply severity overrides if configured
                    if let Some(ref pc) = path_config {
                        for comment in &mut comments {
                            for (category, severity) in &pc.severity_overrides {
                                if comment.category.as_str() == category.to_lowercase() {
                                    comment.severity = match severity.to_lowercase().as_str() {
                                        "error" => core::comment::Severity::Error,
                                        "warning" => core::comment::Severity::Warning,
                                        "info" => core::comment::Severity::Info,
                                        "suggestion" => core::comment::Severity::Suggestion,
                                        _ => comment.severity.clone(),
                                    };
                                }
                            }
                        }
                    }
                    let comments = apply_rule_overrides(comments, &active_rules);

                    let mut comments = filter_comments_for_diff(diff, comments);
                    comments.extend(deterministic_comments);
                    let comment_count = comments.len();

                    // Track comments_by_pass
                    let pass_tag = pass_kind
                        .map(|k| k.tag().to_string())
                        .unwrap_or_else(|| "default".to_string());
                    *comments_by_pass.entry(pass_tag).or_insert(0) += comment_count;

                    // Build per-file metric; merge with existing entry for same file if multi-pass
                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        comment_count,
                    );

                    all_comments.extend(comments);
                } else {
                    // Parse failed; still record file metric
                    let analyzer_comment_count = deterministic_comments.len();
                    merge_file_metric(
                        &mut file_metrics,
                        &file_path,
                        latency_ms,
                        resp_prompt_tokens,
                        resp_completion_tokens,
                        resp_total_tokens,
                        analyzer_comment_count,
                    );
                    all_comments.extend(deterministic_comments);
                }
            }
        }

        if mark_file_complete {
            files_completed += 1;
            if let Some(ref cb) = on_progress_ref {
                cb(ProgressUpdate {
                    current_file: file_path.display().to_string(),
                    files_total,
                    files_completed,
                    files_skipped: files_skipped_snapshot,
                    comments_so_far: all_comments.clone(),
                });
            }
        }
    }

    // Deduplicate across specialized passes when multi-pass is enabled.
    if config.multi_pass_specialized {
        let before = all_comments.len();
        all_comments = deduplicate_specialized_comments(all_comments);
        let after = all_comments.len();
        if before != after {
            info!(
                "Deduplicated {} comment(s) across specialized passes ({} -> {})",
                before - after,
                before,
                after
            );
        }
    }

    // Run post-processors to filter and refine comments
    let processed_comments = plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;

    let (analyzer_comments, llm_comments): (Vec<_>, Vec<_>) = processed_comments
        .into_iter()
        .partition(is_analyzer_comment);

    // Verification pass: ask the LLM to validate findings against actual code.
    // Skip when disabled, no comments, or too many (cost control).
    let verified_llm_comments = if config.verification_pass
        && !llm_comments.is_empty()
        && llm_comments.len() <= config.verification_max_comments
    {
        let comment_count_before = llm_comments.len();
        match super::verification::verify_comments(
            llm_comments,
            &diffs,
            &source_files,
            verification_adapter.as_ref(),
            config.verification_min_score,
        )
        .await
        {
            Ok(verified) => {
                info!(
                    "Verification pass: {}/{} comments passed",
                    verified.len(),
                    comment_count_before
                );
                verified
            }
            Err(e) => {
                warn!(
                    "Verification pass failed, dropping unverified LLM comments: {}",
                    e
                );
                Vec::new()
            }
        }
    } else {
        llm_comments
    };

    let mut processed_comments = analyzer_comments;
    processed_comments.extend(verified_llm_comments);

    let processed_comments = if config.semantic_feedback {
        apply_semantic_feedback_adjustment(
            processed_comments,
            semantic_feedback_store.as_ref(),
            embedding_adapter.as_deref(),
            &config,
        )
        .await
    } else {
        processed_comments
    };

    // Apply feedback-adjusted confidence scores before filtering
    let processed_comments = if config.enhanced_feedback {
        super::filters::apply_feedback_confidence_adjustment(
            processed_comments,
            &feedback,
            config.feedback_min_observations,
        )
    } else {
        processed_comments
    };

    let processed_comments = apply_review_filters(processed_comments, &config, &feedback);

    // Apply enhanced filters from convention learning and composable pipeline
    let processed_comments = core::apply_enhanced_filters(&mut enhanced_ctx, processed_comments);

    // Apply convention-based suppression: filter out comments matching suppression patterns
    let (processed_comments, convention_suppressed_count) =
        apply_convention_suppression(processed_comments, &enhanced_ctx.convention_store);

    // Save updated convention store back to disk
    if let Some(ref store_path) = convention_store_path {
        save_convention_store(&enhanced_ctx.convention_store, store_path);
    }

    Ok(ReviewResult {
        comments: processed_comments,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        file_metrics,
        convention_suppressed_count,
        comments_by_pass,
        hotspots: enhanced_ctx.hotspots.clone(),
        agent_activity: if has_agent_activity {
            Some(AgentActivity {
                total_iterations: aggregate_agent_iterations,
                tool_calls: aggregate_agent_tool_calls,
            })
        } else {
            None
        },
    })
}

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();
    static SYMBOL_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\b([A-Z][a-zA-Z0-9_]*|[a-z][a-zA-Z0-9_]*)\s*\(").unwrap());
    static CLASS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\b(class|struct|interface|enum)\s+([A-Z][a-zA-Z0-9_]*)").unwrap()
    });

    for hunk in &diff.hunks {
        for line in &hunk.changes {
            if matches!(
                line.change_type,
                core::diff_parser::ChangeType::Added | core::diff_parser::ChangeType::Removed
            ) {
                // Extract function calls and references
                for capture in SYMBOL_REGEX.captures_iter(&line.content) {
                    if let Some(symbol) = capture.get(1) {
                        let symbol_str = symbol.as_str().to_string();
                        if symbol_str.len() > 2 && seen.insert(symbol_str.clone()) {
                            symbols.push(symbol_str);
                        }
                    }
                }

                // Also look for class/struct references
                for capture in CLASS_REGEX.captures_iter(&line.content) {
                    if let Some(class_name) = capture.get(2) {
                        let class_str = class_name.as_str().to_string();
                        if seen.insert(class_str.clone()) {
                            symbols.push(class_str);
                        }
                    }
                }
            }
        }
    }

    symbols
}

/// Deduplicate comments that appear in multiple specialized passes.
/// When multi-pass review is enabled, the same issue may be flagged by both
/// the security and correctness passes. We merge near-identical comments,
/// keeping the one with the highest confidence and combining their tags.
fn deduplicate_specialized_comments(mut comments: Vec<core::Comment>) -> Vec<core::Comment> {
    if comments.len() <= 1 {
        return comments;
    }
    // Sort by file_path then line_number for stable dedup
    comments.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });

    let mut deduped: Vec<core::Comment> = Vec::with_capacity(comments.len());
    for comment in comments {
        let dominated = deduped.iter_mut().find(|existing| {
            existing.file_path == comment.file_path
                && existing.line_number == comment.line_number
                && core::multi_pass::content_similarity(&existing.content, &comment.content) > 0.6
        });
        if let Some(existing) = dominated {
            // Merge: keep higher confidence, combine tags
            if comment.confidence > existing.confidence {
                existing.content = comment.content;
                existing.confidence = comment.confidence;
                existing.severity = comment.severity;
            }
            for tag in &comment.tags {
                if !existing.tags.contains(tag) {
                    existing.tags.push(tag.clone());
                }
            }
        } else {
            deduped.push(comment);
        }
    }
    deduped
}

pub fn filter_comments_for_diff(
    diff: &core::UnifiedDiff,
    comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    let mut filtered = Vec::new();
    let total = comments.len();
    for comment in comments {
        if is_line_in_diff(diff, comment.line_number) {
            filtered.push(comment);
        }
    }

    if filtered.len() != total {
        let dropped = total.saturating_sub(filtered.len());
        info!(
            "Dropped {} comment(s) for {} due to unmatched line numbers",
            dropped,
            diff.file_path.display()
        );
    }

    filtered
}

fn synthesize_analyzer_comments(
    findings: Vec<plugins::AnalyzerFinding>,
) -> Result<Vec<core::Comment>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }

    let raw_comments = findings
        .into_iter()
        .map(|finding| finding.into_raw_comment())
        .collect::<Vec<_>>();
    core::CommentSynthesizer::synthesize(raw_comments)
}

fn is_analyzer_comment(comment: &core::Comment) -> bool {
    comment.tags.iter().any(|tag| tag.starts_with("source:"))
}

async fn apply_semantic_feedback_adjustment(
    comments: Vec<core::Comment>,
    store: Option<&core::SemanticFeedbackStore>,
    embedding_adapter: Option<&dyn adapters::llm::LLMAdapter>,
    config: &config::Config,
) -> Vec<core::Comment> {
    let Some(store) = store else {
        return comments;
    };
    if store.examples.len() < config.semantic_feedback_min_examples {
        return comments;
    }

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(embedding_adapter, &embedding_texts).await;

    comments
        .into_iter()
        .zip(embeddings)
        .map(|(mut comment, embedding)| {
            if is_analyzer_comment(&comment) {
                return comment;
            }

            let file_patterns = derive_file_patterns(&comment.file_path);
            let matches = core::find_similar_feedback_examples(
                store,
                &embedding,
                comment.category.as_str(),
                &file_patterns,
                config.semantic_feedback_similarity,
                config.semantic_feedback_max_neighbors,
            );
            let accepted = matches
                .iter()
                .filter(|(example, _)| example.accepted)
                .count();
            let rejected = matches
                .iter()
                .filter(|(example, _)| !example.accepted)
                .count();
            let observations = accepted + rejected;

            if observations < config.semantic_feedback_min_examples {
                return comment;
            }

            if rejected > accepted {
                let delta = ((rejected - accepted) as f32 * 0.15).min(0.45);
                comment.confidence = (comment.confidence - delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:rejected")
                {
                    comment.tags.push("semantic-feedback:rejected".to_string());
                }
            } else if accepted > rejected {
                let delta = ((accepted - rejected) as f32 * 0.10).min(0.25);
                comment.confidence = (comment.confidence + delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:accepted")
                {
                    comment.tags.push("semantic-feedback:accepted".to_string());
                }
            }

            comment
        })
        .collect()
}

pub fn is_line_in_diff(diff: &core::UnifiedDiff, line_number: usize) -> bool {
    if line_number == 0 {
        return false;
    }
    diff.hunks.iter().any(|hunk| {
        hunk.changes
            .iter()
            .any(|line| line.new_line_no == Some(line_number))
    })
}

pub fn build_review_guidance(
    config: &config::Config,
    path_config: Option<&config::PathConfig>,
) -> Option<String> {
    let mut sections = Vec::new();

    let strictness_guidance = match config.strictness {
        1 => "Prefer high-signal findings only. Avoid low-impact nitpicks and optional suggestions.",
        3 => {
            "Be exhaustive. Surface meaningful edge cases and maintainability concerns, including lower-severity findings."
        }
        _ => "Balance precision and coverage; prioritize clear, actionable findings.",
    };
    sections.push(format!(
        "Strictness ({}): {}",
        config.strictness, strictness_guidance
    ));
    if !config.comment_types.is_empty() {
        sections.push(format!(
            "Enabled comment types: {}. Do not emit findings outside these types.",
            config.comment_types.join(", ")
        ));
    }

    if let Some(profile) = config.review_profile.as_deref() {
        let guidance = match profile {
            "chill" => Some(
                "Be conservative and only surface high-confidence, high-impact issues. Avoid nitpicks and redundant comments.",
            ),
            "assertive" => Some(
                "Be thorough and proactive. Surface edge cases, latent risks, and maintainability concerns even if they are subtle.",
            ),
            _ => None,
        };
        if let Some(text) = guidance {
            sections.push(format!("Review profile ({}): {}", profile, text));
        }
    }

    if let Some(instructions) = config.review_instructions.as_deref() {
        let trimmed = instructions.trim();
        if !trimmed.is_empty() {
            sections.push(format!("Global review instructions:\n{}", trimmed));
        }
    }

    if let Some(pc) = path_config {
        if let Some(instructions) = pc.review_instructions.as_deref() {
            let trimmed = instructions.trim();
            if !trimmed.is_empty() {
                sections.push(format!("Path-specific instructions:\n{}", trimmed));
            }
        }
    }

    // Output language directive
    if let Some(ref lang) = config.output_language {
        if lang != "en" && !lang.starts_with("en-") {
            sections.push(format!(
                "Write all review comments and suggestions in {}.",
                lang
            ));
        }
    }

    // Fix suggestions toggle
    if !config.include_fix_suggestions {
        sections.push("Do not include code fix suggestions. Only describe the issue. Do not include <<<ORIGINAL/>>>SUGGESTED blocks.".to_string());
    } else {
        sections.push(
            "For every finding where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:\n\n<<<ORIGINAL\n<the problematic code>\n===\n<the fixed code>\n>>>SUGGESTED\n\nAlways copy the original code verbatim from the diff. Only omit the block when no concrete fix can be expressed in code.".to_string(),
        );
    }

    if sections.is_empty() {
        None
    } else {
        Some(format!(
            "Additional review guidance:\n{}",
            sections.join("\n\n")
        ))
    }
}

pub fn build_symbol_index(config: &config::Config, repo_root: &Path) -> Option<core::SymbolIndex> {
    if !config.symbol_index {
        return None;
    }

    let provider = config.symbol_index_provider.as_str();
    let result = if provider == "lsp" {
        let detected_command = if config.symbol_index_lsp_command.is_none() {
            core::SymbolIndex::detect_lsp_command(
                repo_root,
                config.symbol_index_max_files,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            )
        } else {
            None
        };

        let command = config
            .symbol_index_lsp_command
            .as_deref()
            .map(str::to_string)
            .or(detected_command);

        if let Some(command) = command {
            if config.symbol_index_lsp_command.is_none() {
                info!("Auto-detected LSP command: {}", command);
            }

            match core::SymbolIndex::build_with_lsp(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                &command,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            ) {
                Ok(index) => Ok(index),
                Err(err) => {
                    warn!("LSP indexer failed (falling back to regex): {}", err);
                    core::SymbolIndex::build(
                        repo_root,
                        config.symbol_index_max_files,
                        config.symbol_index_max_bytes,
                        config.symbol_index_max_locations,
                        |path| config.should_exclude(path),
                    )
                }
            }
        } else {
            warn!("No LSP command configured or detected; falling back to regex indexer.");
            core::SymbolIndex::build(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                |path| config.should_exclude(path),
            )
        }
    } else {
        core::SymbolIndex::build(
            repo_root,
            config.symbol_index_max_files,
            config.symbol_index_max_bytes,
            config.symbol_index_max_locations,
            |path| config.should_exclude(path),
        )
    };

    match result {
        Ok(index) => {
            info!(
                "Indexed {} symbols across {} files",
                index.symbols_indexed(),
                index.files_indexed()
            );
            Some(index)
        }
        Err(err) => {
            warn!("Symbol index build failed: {}", err);
            None
        }
    }
}

/// Split a large diff into chunks that fit within context budget.
/// Each chunk gets its own LLM call, results are merged.
fn chunk_diff_for_context(diff_content: &str, max_chars: usize) -> Vec<String> {
    if diff_content.len() <= max_chars {
        return vec![diff_content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    // Split by file boundaries (diff --git)
    for section in diff_content.split("\ndiff --git ") {
        let section = if chunks.is_empty() && current_chunk.is_empty() {
            section.to_string()
        } else {
            format!("diff --git {}", section)
        };

        if current_chunk.len() + section.len() > max_chars && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = section;
        } else {
            current_chunk.push_str(&section);
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

/// Validate LLM response quality for common local model issues.
fn validate_llm_response(response: &str) -> Result<(), String> {
    let trimmed = response.trim();

    // Empty response
    if trimmed.is_empty() {
        return Err("Empty response from model".to_string());
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if is_structured_review_payload(&value) {
            return Ok(());
        }

        return Err("JSON response did not match the review output contract".to_string());
    }

    // Response too short to contain valid review
    if response.len() < 10 {
        return Err("Response too short to contain valid review".to_string());
    }

    // Repeated token detection (common with small models)
    if has_excessive_repetition(response) {
        return Err("Response contains excessive repetition (model may be stuck)".to_string());
    }

    Ok(())
}

fn is_structured_review_payload(value: &serde_json::Value) -> bool {
    let items = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value
        .get("comments")
        .or_else(|| value.get("findings"))
        .or_else(|| value.get("results"))
        .and_then(|items| items.as_array())
    {
        array
    } else {
        return false;
    };

    items.iter().all(|item| {
        item.is_object()
            && (item.get("line").is_some()
                || item.get("line_number").is_some()
                || item.get("content").is_some()
                || item.get("issue").is_some())
    })
}

fn has_excessive_repetition(text: &str) -> bool {
    // Check if any 20-char substring repeats more than 5 times
    if text.len() < 100 {
        return false;
    }
    let window = 20.min(text.len() / 5);
    let search_end = text.len().saturating_sub(window);
    for start in 0..search_end.max(1) {
        if !text.is_char_boundary(start) || !text.is_char_boundary(start + window) {
            continue;
        }
        let pattern = &text[start..start + window];
        if pattern.trim().is_empty() {
            continue;
        }
        let count = text.matches(pattern).count();
        if count > 5 {
            return true;
        }
    }
    false
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

fn merge_file_metric(
    file_metrics: &mut Vec<FileMetric>,
    file_path: &Path,
    latency_ms: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
    comment_count: usize,
) {
    if let Some(existing) = file_metrics
        .iter_mut()
        .find(|metric| metric.file_path == file_path)
    {
        existing.prompt_tokens += prompt_tokens;
        existing.completion_tokens += completion_tokens;
        existing.total_tokens += total_tokens;
        existing.comment_count += comment_count;
        if latency_ms > existing.latency_ms {
            existing.latency_ms = latency_ms;
        }
        return;
    }

    file_metrics.push(FileMetric {
        file_path: file_path.to_path_buf(),
        latency_ms,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        comment_count,
    });
}

/// Auto-detect instruction files commonly used by AI coding tools.
/// Returns the concatenated contents of any found files (.cursorrules, CLAUDE.md, etc.)
fn detect_instruction_files(repo_path: &Path) -> Vec<(String, String)> {
    const INSTRUCTION_FILES: &[&str] = &[
        ".cursorrules",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "agents.md",
        ".github/copilot-instructions.md",
        "GEMINI.md",
        ".diffscope-instructions.md",
    ];
    const MAX_INSTRUCTION_SIZE: u64 = 10_000;

    let mut results = Vec::new();
    for filename in INSTRUCTION_FILES {
        let path = repo_path.join(filename);
        if path.is_file() {
            // Skip files larger than 10KB
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > MAX_INSTRUCTION_SIZE {
                    warn!(
                        "Skipping instruction file {} ({} bytes exceeds {})",
                        filename,
                        meta.len(),
                        MAX_INSTRUCTION_SIZE
                    );
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    info!("Auto-detected instruction file: {}", filename);
                    results.push((filename.to_string(), trimmed));
                }
            }
        }
    }
    results
}

fn should_optimize_for_local(config: &config::Config) -> bool {
    // Optimize if context_window is explicitly set
    if config.context_window.is_some() {
        return true;
    }
    // Optimize for ollama: prefix models
    if config.model.starts_with("ollama:") {
        return true;
    }
    // Optimize if adapter is explicitly set to ollama
    if config.adapter.as_deref() == Some("ollama") {
        return true;
    }
    // Optimize if base_url points to localhost
    config.is_local_endpoint()
}

/// Run `git log --numstat` against repo_path to gather commit history.
/// Returns None if the command fails (e.g., not a git repo).
fn gather_git_log(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--numstat",
            "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ai%n%n    %s%n",
            "-100",
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let log_text = String::from_utf8_lossy(&out.stdout).to_string();
            if log_text.trim().is_empty() {
                None
            } else {
                info!("Gathered git log ({} bytes)", log_text.len());
                Some(log_text)
            }
        }
        _ => {
            info!("Git log unavailable (not a git repo or git not found)");
            None
        }
    }
}

/// Resolve the convention store path from config or default location.
fn resolve_convention_store_path(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    // Default: ~/.local/share/diffscope/conventions.json
    dirs::data_local_dir().map(|d| d.join("diffscope").join("conventions.json"))
}

/// Save the convention store to the given path, creating directories if needed.
fn save_convention_store(store: &ConventionStore, path: &PathBuf) {
    if let Ok(json) = store.to_json() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(path, json) {
            warn!(
                "Failed to save convention store to {}: {}",
                path.display(),
                e
            );
        }
    }
}

/// Gather related file context: reverse dependencies (callers) and test files.
/// These are included as Reference context chunks for the LLM.
fn gather_related_file_context(
    index: &core::SymbolIndex,
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    let mut chunks: Vec<core::LLMContextChunk> = Vec::new();

    // 1. Reverse dependencies (files that import/depend on this file)
    let callers = index.reverse_deps(file_path);
    for caller_path in callers.iter().take(3) {
        if let Some(summary) = index.file_summary(caller_path) {
            let truncated: String = if summary.len() > 2000 {
                let mut end = 2000;
                while end > 0 && !summary.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &summary[..end])
            } else {
                summary.to_string()
            };
            chunks.push(core::LLMContextChunk {
                file_path: caller_path.clone(),
                content: format!("[Caller/dependent file]\n{}", truncated),
                context_type: core::ContextType::Reference,
                line_range: None,
            });
        }
    }

    // 2. Look for matching test files
    let test_files = find_test_files(file_path, repo_path);
    for test_path in test_files.iter().take(2) {
        let relative: &Path = test_path.strip_prefix(repo_path).unwrap_or(test_path);
        // Read first 60 lines of the test file for context
        if let Ok(content) = std::fs::read_to_string(test_path) {
            let snippet: String = content.lines().take(60).collect::<Vec<_>>().join("\n");
            if !snippet.is_empty() {
                chunks.push(core::LLMContextChunk {
                    file_path: relative.to_path_buf(),
                    content: format!("[Test file]\n{}", snippet),
                    context_type: core::ContextType::Reference,
                    line_range: None,
                });
            }
        }
    }

    chunks
}

/// Find test files that correspond to a given source file.
/// Looks for patterns like test_<stem>, <stem>_test, <stem>.test, tests/<stem>.
fn find_test_files(file_path: &Path, repo_path: &Path) -> Vec<PathBuf> {
    let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = file_path.parent().unwrap_or(Path::new(""));

    let candidates: Vec<PathBuf> = vec![
        repo_path
            .join(parent)
            .join(format!("test_{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}_test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.spec.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("tests")
            .join(format!("{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("__tests__")
            .join(format!("{}.{}", stem, ext)),
    ];

    candidates
        .into_iter()
        .filter(|p: &PathBuf| p.is_file())
        .collect()
}

/// Apply convention-based suppression: filter out comments whose content
/// matches learned suppression patterns with high confidence.
/// Returns the filtered comments and the number of comments that were suppressed.
fn apply_convention_suppression(
    comments: Vec<core::Comment>,
    convention_store: &ConventionStore,
) -> (Vec<core::Comment>, usize) {
    let suppression_patterns = convention_store.suppression_patterns();
    if suppression_patterns.is_empty() {
        return (comments, 0);
    }

    let before_count = comments.len();
    let filtered: Vec<core::Comment> = comments
        .into_iter()
        .filter(|comment| {
            let category_str = comment.category.to_string();
            let score = convention_store.score_comment(&comment.content, &category_str);
            // Only suppress if the convention store strongly indicates suppression
            // (score <= -0.25 means the team has consistently rejected similar comments)
            score > -0.25
        })
        .collect();

    let suppressed = before_count.saturating_sub(filtered.len());
    if suppressed > 0 {
        info!(
            "Convention learning suppressed {} comment(s) based on team feedback patterns",
            suppressed
        );
    }

    (filtered, suppressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_symbols_from_diff_finds_functions() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "let result = process_data(input);".to_string(),
                    change_type: core::diff_parser::ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.contains(&"process_data".to_string()));
    }

    #[test]
    fn extract_symbols_from_diff_finds_classes() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "struct MyHandler {".to_string(),
                    change_type: core::diff_parser::ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.contains(&"MyHandler".to_string()));
    }

    #[test]
    fn extract_symbols_ignores_context_lines() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "let x = unchanged_func(y);".to_string(),
                    change_type: core::diff_parser::ChangeType::Context,
                    old_line_no: Some(1),
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.is_empty());
    }

    #[test]
    fn extract_symbols_preserves_insertion_order() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 3,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "alpha(1);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "beta(2);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                    core::diff_parser::DiffLine {
                        content: "gamma(3); alpha(4);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(3),
                    },
                ],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        // Deduplicates alpha, preserves first-seen order
        let positions: Vec<usize> = ["alpha", "beta", "gamma"]
            .iter()
            .map(|s| symbols.iter().position(|x| x == s).unwrap())
            .collect();
        assert!(positions[0] < positions[1]);
        assert!(positions[1] < positions[2]);
        assert_eq!(symbols.iter().filter(|s| *s == "alpha").count(), 1);
    }

    #[test]
    fn extract_symbols_deduplicates() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 2,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "foo(1);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "foo(2);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                ],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert_eq!(symbols.iter().filter(|s| *s == "foo").count(), 1);
    }

    #[test]
    fn is_line_in_diff_basic() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "changed".to_string(),
                    change_type: core::diff_parser::ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(5),
                }],
            }],
        };
        assert!(is_line_in_diff(&diff, 5));
        assert!(!is_line_in_diff(&diff, 6));
        assert!(!is_line_in_diff(&diff, 0));
    }

    #[test]
    fn build_review_guidance_includes_strictness() {
        let config = config::Config {
            strictness: 1,
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("high-signal"));
    }

    #[test]
    fn build_review_guidance_includes_comment_types() {
        let config = config::Config {
            comment_types: vec!["logic".to_string(), "syntax".to_string()],
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("logic, syntax"));
    }

    #[test]
    fn build_review_guidance_includes_profile() {
        let config = config::Config {
            review_profile: Some("assertive".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("thorough"));
    }

    #[test]
    fn build_review_guidance_includes_path_instructions() {
        let config = config::Config::default();
        let path_config = config::PathConfig {
            review_instructions: Some("Be extra careful here".to_string()),
            ..config::PathConfig::default()
        };
        let guidance = build_review_guidance(&config, Some(&path_config)).unwrap();
        assert!(guidance.contains("Be extra careful here"));
    }

    #[test]
    fn build_review_guidance_includes_output_language() {
        let config = config::Config {
            output_language: Some("ja".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("ja"));
    }

    #[test]
    fn build_review_guidance_skips_en_language() {
        let config = config::Config {
            output_language: Some("en".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        // "en" language should not add a language directive
        assert!(!guidance.contains("Write all review comments"));
    }

    #[test]
    fn build_review_guidance_skips_en_us_language() {
        let config = config::Config {
            output_language: Some("en-us".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(!guidance.contains("Write all review comments"));
    }

    #[test]
    fn build_review_guidance_no_fix_suggestions() {
        let config = config::Config {
            include_fix_suggestions: false,
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Do not include code fix suggestions"));
    }

    #[test]
    fn detect_instruction_files_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = detect_instruction_files(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn detect_instruction_files_finds_cursorrules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".cursorrules"), "Use tabs not spaces").unwrap();
        let results = detect_instruction_files(dir.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, ".cursorrules");
        assert!(results[0].1.contains("Use tabs"));
    }

    // --- chunk_diff_for_context tests ---

    #[test]
    fn chunk_diff_small_diff_returns_single_chunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+hello\n";
        let chunks = chunk_diff_for_context(diff, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], diff);
    }

    #[test]
    fn chunk_diff_splits_at_file_boundaries() {
        let diff = "diff --git a/a.rs b/a.rs\n+line1\n\ndiff --git a/b.rs b/b.rs\n+line2\n\ndiff --git a/c.rs b/c.rs\n+line3\n";
        // Set max_chars small enough to force splits
        let chunks = chunk_diff_for_context(diff, 40);
        assert!(
            chunks.len() >= 2,
            "Expected at least 2 chunks, got {}",
            chunks.len()
        );
        // Each chunk should start with diff --git (or be the first chunk which inherits it)
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(
                chunk.contains("diff --git"),
                "Chunk {} should contain 'diff --git': {:?}",
                i,
                chunk
            );
        }
    }

    #[test]
    fn chunk_diff_empty_input() {
        let chunks = chunk_diff_for_context("", 100);
        // Empty string produces one chunk containing the empty string
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn chunk_diff_single_large_file_not_split_midfile() {
        // A single file diff that exceeds max_chars should still be one chunk
        // (we only split at file boundaries, not mid-file)
        let diff = format!("diff --git a/big.rs b/big.rs\n{}", "+line\n".repeat(100));
        let chunks = chunk_diff_for_context(&diff, 50);
        assert_eq!(chunks.len(), 1, "Single-file diff should not be split");
    }

    #[test]
    fn chunk_diff_preserves_all_content() {
        let file_a = "diff --git a/a.rs b/a.rs\n+alpha\n";
        let file_b = "\ndiff --git a/b.rs b/b.rs\n+beta\n";
        let file_c = "\ndiff --git a/c.rs b/c.rs\n+gamma\n";
        let diff = format!("{}{}{}", file_a, file_b, file_c);
        let chunks = chunk_diff_for_context(&diff, 50);
        // Rejoin all chunks and verify content is preserved
        let rejoined: String = chunks.join("");
        // Both original and rejoined should contain the same file sections
        assert!(rejoined.contains("+alpha"));
        assert!(rejoined.contains("+beta"));
        assert!(rejoined.contains("+gamma"));
    }

    // --- validate_llm_response tests ---

    #[test]
    fn validate_response_accepts_valid_response() {
        let response = "Here is my review of the code changes:\n- Line 5: potential null reference";
        assert!(validate_llm_response(response).is_ok());
    }

    #[test]
    fn validate_response_accepts_structured_json() {
        assert!(validate_llm_response("[]").is_ok());
        assert!(validate_llm_response("[{\"line\":10,\"issue\":\"problem\"}]").is_ok());
    }

    #[test]
    fn validate_response_rejects_empty() {
        assert!(validate_llm_response("").is_err());
        assert!(validate_llm_response("   \n\t  ").is_err());
    }

    #[test]
    fn validate_response_rejects_too_short() {
        assert!(validate_llm_response("OK").is_err());
        assert!(validate_llm_response("no issue").is_err());
    }

    #[test]
    fn validate_response_rejects_repetitive() {
        // Create a response with excessive repetition
        let repeated = "This is a repeating segment.".repeat(20);
        assert!(validate_llm_response(&repeated).is_err());
    }

    // --- has_excessive_repetition tests ---

    #[test]
    fn repetition_short_text_always_false() {
        assert!(!has_excessive_repetition("short"));
        assert!(!has_excessive_repetition(""));
        assert!(!has_excessive_repetition("a".repeat(99).as_str()));
    }

    #[test]
    fn repetition_normal_text_false() {
        let text = "This is a normal code review response. The function looks correct \
                    but there may be an edge case on line 42 where the input could be null. \
                    Consider adding a guard clause to handle this scenario.";
        assert!(!has_excessive_repetition(text));
    }

    #[test]
    fn repetition_stuck_model_detected() {
        // Simulate a model stuck repeating tokens
        let text = "The code looks fine. ".repeat(10);
        assert!(has_excessive_repetition(&text));
    }

    #[test]
    fn repetition_whitespace_only_not_flagged() {
        // 200 spaces should not be flagged (whitespace patterns are skipped)
        let text = " ".repeat(200);
        assert!(!has_excessive_repetition(&text));
    }

    // --- deduplicate_specialized_comments tests ---

    fn make_comment(file: &str, line: usize, content: &str, tag: &str) -> core::Comment {
        core::Comment {
            id: format!("cmt_{}", line),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::BestPractice,
            suggestion: None,
            confidence: 0.7,
            code_suggestion: None,
            tags: vec![tag.to_string()],
            fix_effort: core::comment::FixEffort::Medium,
            feedback: None,
        }
    }

    #[test]
    fn dedup_removes_similar_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                10,
                "Missing null check on user input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].tags.contains(&"security-pass".to_string()));
    }

    #[test]
    fn dedup_keeps_different_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "SQL injection vulnerability", "security-pass"),
            make_comment("a.rs", 10, "Off-by-one error in loop", "correctness-pass"),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_keeps_similar_comments_on_different_lines() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                20,
                "Missing null check on input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_handles_empty_input() {
        let deduped = deduplicate_specialized_comments(vec![]);
        assert!(deduped.is_empty());
    }

    #[test]
    fn specialized_prompts_are_distinct() {
        let security = core::prompt::build_security_prompt();
        let correctness = core::prompt::build_correctness_prompt();
        let style = core::prompt::build_style_prompt();
        assert!(security.contains("security"));
        assert!(correctness.contains("correctness"));
        assert!(style.contains("style"));
        assert_ne!(security, correctness);
        assert_ne!(security, style);
        assert_ne!(correctness, style);
    }

    #[test]
    fn specialized_pass_kind_tags() {
        assert_eq!(core::SpecializedPassKind::Security.tag(), "security-pass");
        assert_eq!(
            core::SpecializedPassKind::Correctness.tag(),
            "correctness-pass"
        );
        assert_eq!(core::SpecializedPassKind::Style.tag(), "style-pass");
    }

    #[test]
    fn multi_pass_specialized_config_default_false() {
        let config = config::Config::default();
        assert!(!config.multi_pass_specialized);
    }

    #[test]
    fn test_has_excessive_repetition_boundary_120_chars() {
        // BUG: when text.len() == 100, window=20, the old loop range was 0..0 (empty).
        // Even after fixing the range, the check is count > 5, so we need 6 repetitions.
        let pattern = "abcdefghij1234567890"; // 20 chars
        let text = pattern.repeat(6); // 120 chars, 6 repetitions
        assert_eq!(text.len(), 120);
        assert!(
            has_excessive_repetition(&text),
            "120-char string with 6x repeated 20-char pattern should be detected"
        );
    }

    #[test]
    fn test_has_excessive_repetition_short_not_detected() {
        // Strings under 100 chars should always return false
        let text = "abc".repeat(30); // 90 chars
        assert!(!has_excessive_repetition(&text));
    }

    // ── Bug: UTF-8 slicing panic on multi-byte chars at boundary ─────
    //
    // The summary truncation in gather_related_file_context used
    // `&summary[..2000]`, which panics if byte 2000 falls inside a
    // multi-byte UTF-8 character (e.g. emoji, CJK, accented chars).
    // The fix uses is_char_boundary() to find a safe slice point.

    #[test]
    fn test_utf8_safe_truncation() {
        // "€" is 3 bytes in UTF-8. Create a string where byte 2000
        // lands inside a multi-byte char.
        let prefix = "a".repeat(1999); // 1999 bytes
        let s = format!("{}€rest", prefix); // byte 1999-2001 is "€" (3 bytes)
        assert!(s.len() > 2000);

        // This is the pattern from the fix — it should not panic
        let mut end = 2000;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &s[..end];
        // The truncation should stop before the "€" character
        assert_eq!(
            end, 1999,
            "Should back up to byte 1999, before the 3-byte €"
        );
        assert!(truncated.starts_with("aaa"));
        assert!(!truncated.contains('€'));
    }
}
