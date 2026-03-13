use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use tracing::{info, warn};

use std::sync::Arc;

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::plugins;

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

#[path = "pipeline/execution.rs"]
mod execution;
#[path = "pipeline/helpers.rs"]
mod helpers;
#[path = "pipeline/prepare.rs"]
mod prepare;

use super::context_helpers::resolve_pattern_repositories;
use super::feedback::load_feedback_store;
use super::filters::apply_review_filters;
use super::rule_helpers::load_review_rules;
use execution::{execute_review_jobs, ReviewExecutionContext};
use helpers::{
    apply_convention_suppression, apply_semantic_feedback_adjustment, chunk_diff_for_context,
    deduplicate_specialized_comments, detect_instruction_files, gather_git_log,
    is_analyzer_comment, resolve_convention_store_path, save_convention_store,
    should_optimize_for_local,
};
pub use helpers::{
    build_review_guidance, build_symbol_index, extract_symbols_from_diff, filter_comments_for_diff,
    is_line_in_diff,
};
#[cfg(test)]
use helpers::{has_excessive_repetition, validate_llm_response};
use prepare::{prepare_file_review_jobs, ReviewPreparationContext};

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
    let repo_path_str = repo_path.to_string_lossy().to_string();
    let context_fetcher = core::ContextFetcher::new(repo_path.to_path_buf());
    let is_local = should_optimize_for_local(&config);
    let batched_pre_analysis = plugin_manager
        .run_pre_analyzers_for_review(&diffs, &repo_path_str)
        .await?;

    let prepare::PreparedReviewJobs {
        jobs,
        all_comments: prepared_comments,
        verification_context,
        files_completed: prepared_files_completed,
        files_skipped: prepared_files_skipped,
    } = prepare_file_review_jobs(ReviewPreparationContext {
        diffs: &diffs,
        config: &config,
        repo_path,
        on_progress: on_progress.clone(),
        source_files: &source_files,
        context_fetcher: &context_fetcher,
        symbol_index: &symbol_index,
        semantic_index: semantic_index.as_ref(),
        embedding_adapter: embedding_adapter.as_deref(),
        pattern_repositories: &pattern_repositories,
        review_rules: &review_rules,
        feedback_context: &feedback_context,
        base_prompt_config: &base_prompt_config,
        enhanced_guidance: &enhanced_guidance,
        auto_instructions: auto_instructions.as_ref(),
        batched_pre_analysis,
        is_local,
    })
    .await?;

    let files_completed = prepared_files_completed;
    let files_skipped = prepared_files_skipped;

    let execution = execute_review_jobs(
        jobs,
        ReviewExecutionContext {
            diffs: &diffs,
            config: &config,
            repo_path,
            adapter: adapter.clone(),
            is_local,
            on_progress: on_progress.clone(),
            initial_comments: prepared_comments,
            files_total,
            files_completed,
            files_skipped,
        },
    )
    .await?;

    let mut all_comments = execution.all_comments;
    let total_prompt_tokens = execution.total_prompt_tokens;
    let total_completion_tokens = execution.total_completion_tokens;
    let total_tokens = execution.total_tokens;
    let file_metrics = execution.file_metrics;
    let comments_by_pass = execution.comments_by_pass;
    let agent_activity = execution.agent_activity;

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
            &verification_context,
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
        agent_activity,
    })
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
