use anyhow::Result;
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::{build_change_walkthrough, format_smart_review_output};
use crate::parsing::parse_smart_review_response;
use crate::review;

pub async fn smart_review_command(
    config: config::Config,
    diff_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!(
        "Starting smart review analysis with model: {}",
        config.model
    );

    let repo_root = core::GitIntegration::new(".")
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or_else(|| PathBuf::from("."));
    let repo_path_str = repo_root.to_string_lossy().to_string();
    let context_fetcher = core::ContextFetcher::new(repo_root.clone());
    let feedback = review::load_feedback_store(&config);

    let mut plugin_manager = crate::plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;

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
    let walkthrough = build_change_walkthrough(&diffs);
    let symbol_index = review::build_symbol_index(&config, &repo_root);
    let pattern_repositories = review::resolve_pattern_repositories(&config, &repo_root);
    let review_rules = review::load_review_rules(&config, &pattern_repositories, &repo_root);

    let model_config = config.to_model_config();

    let adapter = adapters::llm::create_adapter(&model_config)?;

    // Use Fast model for PR summary and diagram generation (lightweight tasks).
    // Only create a separate adapter if model_fast differs from the primary model.
    let fast_config = config.to_model_config_for_role(config::ModelRole::Fast);
    let separate_fast_adapter: Option<Box<dyn adapters::llm::LLMAdapter>> =
        if fast_config.model_name != model_config.model_name {
            info!(
                "Using fast model '{}' for PR summary/diagram",
                fast_config.model_name
            );
            Some(adapters::llm::create_adapter(&fast_config)?)
        } else {
            None
        };
    let summary_adapter: &dyn adapters::llm::LLMAdapter =
        separate_fast_adapter.as_deref().unwrap_or(adapter.as_ref());

    let mut all_comments = Vec::new();
    let mut pr_summary = if config.smart_review_summary {
        match core::GitIntegration::new(&repo_root) {
            Ok(git) => {
                let options = core::SummaryOptions {
                    include_diagram: false,
                };
                match core::PRSummaryGenerator::generate_summary_with_options(
                    &diffs,
                    &git,
                    summary_adapter,
                    options,
                )
                .await
                {
                    Ok(summary) => Some(summary),
                    Err(err) => {
                        warn!("PR summary generation failed: {}", err);
                        None
                    }
                }
            }
            Err(err) => {
                warn!("Skipping PR summary (git unavailable): {}", err);
                None
            }
        }
    } else {
        None
    };

    if config.smart_review_diagram {
        match core::PRSummaryGenerator::generate_change_diagram(&diffs, summary_adapter).await {
            Ok(Some(diagram)) => {
                if let Some(summary) = &mut pr_summary {
                    summary.visual_diff = Some(diagram);
                } else {
                    pr_summary = Some(core::PRSummaryGenerator::build_diagram_only_summary(
                        &diffs, diagram,
                    ));
                }
            }
            Ok(None) => {}
            Err(err) => warn!("Diagram generation failed: {}", err),
        }
    }

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

        // Get path-specific configuration
        let path_config = config.get_path_config(&diff.file_path);

        // Add focus areas to context if configured
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
        review::inject_custom_context(&config, &context_fetcher, diff, &mut context_chunks).await?;
        review::inject_pattern_repository_context(
            &config,
            &pattern_repositories,
            &context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;

        // Extract symbols and get definitions
        let symbols = review::extract_symbols_from_diff(diff);
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

        let active_rules =
            core::active_rules_for_file(&review_rules, &diff.file_path, config.max_active_rules);
        review::inject_rule_context(diff, &active_rules, &mut context_chunks);
        context_chunks = review::rank_and_trim_context_chunks(
            diff,
            context_chunks,
            config.context_max_chunks,
            config.context_budget_chars,
        );

        let guidance = review::build_review_guidance(&config, path_config);
        let (system_prompt, user_prompt) =
            core::SmartReviewPromptBuilder::build_enhanced_review_prompt(
                diff,
                &context_chunks,
                config.max_context_chars,
                config.max_diff_chars,
                guidance.as_deref(),
            )?;

        let request = adapters::llm::LLMRequest {
            system_prompt,
            user_prompt,
            temperature: Some(0.2),
            max_tokens: Some(4000),
        };

        let response = adapter.complete(request).await?;

        if let Ok(raw_comments) = parse_smart_review_response(&response.content, &diff.file_path) {
            let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;

            // Apply severity overrides if configured
            if let Some(pc) = path_config {
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

            let comments = review::apply_rule_overrides(comments, &active_rules);
            let comments = review::filter_comments_for_diff(diff, comments);
            all_comments.extend(comments);
        }
    }

    // Run post-processors to filter and refine comments
    let processed_comments = plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;
    let processed_comments = review::apply_review_filters(processed_comments, &config, &feedback);

    // Generate summary and output results
    let summary = core::CommentSynthesizer::generate_summary(&processed_comments);
    let output = format_smart_review_output(
        &processed_comments,
        &summary,
        pr_summary.as_ref(),
        &walkthrough,
        &config.rule_priority,
    );

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
    } else {
        println!("{}", output);
    }

    Ok(())
}
