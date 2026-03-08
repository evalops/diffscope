use anyhow::Result;
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing::info;

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::{format_diff_as_unified, OutputFormat};
use crate::review::{review_diff_content, review_diff_content_with_repo};

pub async fn review_command(
    config: config::Config,
    diff_path: Option<PathBuf>,
    patch: bool,
    output_path: Option<PathBuf>,
    format: OutputFormat,
) -> Result<()> {
    info!("Starting diff review with model: {}", config.model);

    let repo_root = core::GitIntegration::new(".")
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or_else(|| PathBuf::from("."));
    let repo_path_str = repo_root.to_string_lossy().to_string();
    let context_fetcher = core::ContextFetcher::new(repo_root.clone());
    let pattern_repositories = crate::review::resolve_pattern_repositories(&config, &repo_root);
    let review_rules = crate::review::load_review_rules(&config, &pattern_repositories, &repo_root);

    let mut plugin_manager = crate::plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;
    let feedback = crate::review::load_feedback_store(&config);

    let diff_content = if let Some(path) = diff_path {
        tokio::fs::read_to_string(path).await?
    } else if std::io::stdin().is_terminal() {
        if let Ok(git) = core::GitIntegration::new(".") {
            let diff = git.get_uncommitted_diff()?;
            if diff.is_empty() {
                println!("No changes found");
                return Ok(());
            }
            diff
        } else {
            println!("No diff provided and not in a git repository.");
            return Ok(());
        }
    } else {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
    info!("Parsed {} file diffs", diffs.len());
    let symbol_index = crate::review::build_symbol_index(&config, &repo_root);
    let model_config = config.to_model_config();

    let adapter = adapters::llm::create_adapter(&model_config)?;
    let base_prompt_config = core::prompt::PromptConfig {
        max_context_chars: config.max_context_chars,
        max_diff_chars: config.max_diff_chars,
        ..Default::default()
    };
    let mut all_comments = Vec::new();

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
        let symbols = crate::review::extract_symbols_from_diff(diff);
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

        // Apply path-specific system prompt if available
        let mut local_prompt_config = base_prompt_config.clone();
        if let Some(custom_prompt) = &config.system_prompt {
            local_prompt_config.system_prompt = custom_prompt.clone();
        }
        if let Some(pc) = path_config {
            if let Some(ref prompt) = pc.system_prompt {
                local_prompt_config.system_prompt = prompt.clone();
            }

            // Add focus areas to context
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
        crate::review::inject_custom_context(&config, &context_fetcher, diff, &mut context_chunks).await?;
        crate::review::inject_pattern_repository_context(
            &config,
            &pattern_repositories,
            &context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;
        let active_rules =
            core::active_rules_for_file(&review_rules, &diff.file_path, config.max_active_rules);
        crate::review::inject_rule_context(diff, &active_rules, &mut context_chunks);
        context_chunks = crate::review::rank_and_trim_context_chunks(
            diff,
            context_chunks,
            config.context_max_chunks,
            config.context_budget_chars,
        );

        if let Some(guidance) = crate::review::build_review_guidance(&config, path_config) {
            local_prompt_config.system_prompt.push_str("\n\n");
            local_prompt_config.system_prompt.push_str(&guidance);
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

        if let Ok(raw_comments) = crate::parsing::parse_llm_response(&response.content, &diff.file_path) {
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
            let comments = crate::review::apply_rule_overrides(comments, &active_rules);

            let comments = crate::review::filter_comments_for_diff(diff, comments);
            all_comments.extend(comments);
        }
    }

    let processed_comments = plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;
    let processed_comments = crate::review::apply_review_filters(processed_comments, &config, &feedback);

    let effective_format = if patch { OutputFormat::Patch } else { format };
    crate::output::output_comments(
        &processed_comments,
        output_path,
        effective_format,
        &config.rule_priority,
    )
    .await?;

    Ok(())
}

pub async fn check_command(path: PathBuf, config: config::Config, format: OutputFormat) -> Result<()> {
    info!("Checking repository at: {}", path.display());
    info!("Using model: {}", config.model);

    let git = core::GitIntegration::new(&path)?;
    let diff_content = git.get_uncommitted_diff()?;
    if diff_content.is_empty() {
        println!("No changes found in {}", path.display());
        return Ok(());
    }

    let repo_root = git.workdir().unwrap_or(path);
    review_diff_content_with_repo(&diff_content, config, format, &repo_root).await
}

pub async fn compare_command(
    old_file: PathBuf,
    new_file: PathBuf,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    info!(
        "Comparing files: {} vs {}",
        old_file.display(),
        new_file.display()
    );

    let old_content = tokio::fs::read_to_string(&old_file).await?;
    let new_content = tokio::fs::read_to_string(&new_file).await?;

    // Use the parse_text_diff function to create a UnifiedDiff
    let diff = core::DiffParser::parse_text_diff(&old_content, &new_content, new_file.clone())?;

    // Convert the diff to a string format for the review process
    let diff_string = format!(
        "--- {}\n+++ {}\n{}",
        old_file.display(),
        new_file.display(),
        format_diff_as_unified(&diff)
    );

    review_diff_content(&diff_string, config, format).await
}
