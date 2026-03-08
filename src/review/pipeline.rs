use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::parsing::parse_llm_response;
use crate::plugins;
use super::context_helpers::{
    inject_custom_context, inject_pattern_repository_context,
    rank_and_trim_context_chunks, resolve_pattern_repositories,
};
use super::feedback::load_feedback_store;
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
    let comments = review_diff_content_raw(diff_content, config, repo_path).await?;
    crate::output::output_comments(&comments, None, format, &rule_priority).await
}

pub async fn review_diff_content_raw(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
) -> Result<Vec<core::Comment>> {
    let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
    info!("Parsed {} file diffs", diffs.len());

    // Build enhanced review context from the new modules
    let mut enhanced_ctx = core::build_enhanced_context(
        &diffs,
        &HashMap::new(),
        None,
        None,
        None,
        None,
    );
    let enhanced_guidance = core::generate_enhanced_guidance(&enhanced_ctx, "rs");
    if !enhanced_guidance.is_empty() {
        info!("Enhanced guidance generated ({} chars)", enhanced_guidance.len());
    }

    let symbol_index = build_symbol_index(&config, repo_path);
    let pattern_repositories = resolve_pattern_repositories(&config, repo_path);
    let review_rules = load_review_rules(&config, &pattern_repositories, repo_path);

    // Initialize plugin manager and load builtin plugins
    let mut plugin_manager = plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;
    let feedback = load_feedback_store(&config);

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

    let adapter = adapters::llm::create_adapter(&model_config)?;
    let base_prompt_config = core::prompt::PromptConfig {
        max_context_chars: config.max_context_chars,
        max_diff_chars: config.max_diff_chars,
        ..Default::default()
    };
    let mut all_comments = Vec::new();

    let repo_path_str = repo_path.to_string_lossy().to_string();
    let context_fetcher = core::ContextFetcher::new(repo_path.to_path_buf());

    for diff in &diffs {
        // Check if file should be excluded
        if config.should_exclude(&diff.file_path) {
            info!("Skipping excluded file: {}", diff.file_path.display());
            continue;
        }
        if diff.is_deleted {
            info!("Skipping deleted file: {}", diff.file_path.display());
            continue;
        }
        if diff.is_binary || diff.hunks.is_empty() {
            info!("Skipping non-text diff: {}", diff.file_path.display());
            continue;
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

        // Run pre-analyzers to get additional context
        let analyzer_chunks = plugin_manager
            .run_pre_analyzers(diff, &repo_path_str)
            .await?;
        context_chunks.extend(analyzer_chunks);

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

        // Get path-specific configuration
        let path_config = config.get_path_config(&diff.file_path);

        // Add focus areas and extra context if configured
        if let Some(pc) = path_config {
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

        // Create prompt builder with config
        let mut local_prompt_config = base_prompt_config.clone();
        if let Some(custom_prompt) = &config.system_prompt {
            local_prompt_config.system_prompt = custom_prompt.clone();
        }
        if let Some(pc) = path_config {
            if let Some(ref prompt) = pc.system_prompt {
                local_prompt_config.system_prompt = prompt.clone();
            }
        }
        if let Some(guidance) = build_review_guidance(&config, path_config) {
            local_prompt_config.system_prompt.push_str("\n\n");
            local_prompt_config.system_prompt.push_str(&guidance);
        }
        // Inject enhanced guidance from the new modules
        if !enhanced_guidance.is_empty() {
            local_prompt_config.system_prompt.push_str("\n\n");
            local_prompt_config.system_prompt.push_str(&enhanced_guidance);
        }
        let local_prompt_builder = core::PromptBuilder::new(local_prompt_config);
        let (system_prompt, user_prompt) =
            local_prompt_builder.build_prompt(diff, &context_chunks)?;

        let request = adapters::llm::LLMRequest {
            system_prompt,
            user_prompt,
            temperature: None,
            max_tokens: None,
        };

        let response = adapter.complete(request).await?;

        if let Ok(raw_comments) = parse_llm_response(&response.content, &diff.file_path) {
            let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;

            // Apply severity overrides if configured
            if let Some(pc) = path_config {
                for comment in &mut comments {
                    for (category, severity) in &pc.severity_overrides {
                        if format!("{:?}", comment.category).to_lowercase()
                            == category.to_lowercase()
                        {
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

            let comments = filter_comments_for_diff(diff, comments);
            all_comments.extend(comments);
        }
    }

    // Run post-processors to filter and refine comments
    let processed_comments = plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;
    let processed_comments = apply_review_filters(processed_comments, &config, &feedback);

    // Apply enhanced filters from convention learning and composable pipeline
    let processed_comments = core::apply_enhanced_filters(&mut enhanced_ctx, processed_comments);

    Ok(processed_comments)
}

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
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
                        if symbol_str.len() > 2 && !symbols.contains(&symbol_str) {
                            symbols.push(symbol_str);
                        }
                    }
                }

                // Also look for class/struct references
                for capture in CLASS_REGEX.captures_iter(&line.content) {
                    if let Some(class_name) = capture.get(2) {
                        let class_str = class_name.as_str().to_string();
                        if !symbols.contains(&class_str) {
                            symbols.push(class_str);
                        }
                    }
                }
            }
        }
    }

    symbols
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
}
