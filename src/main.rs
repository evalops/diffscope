mod adapters;
mod config;
mod core;
mod plugins;

use anyhow::Result;
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "diffscope")]
#[command(about = "A composable code review engine with smart analysis and professional reporting", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, default_value = "gpt-4o")]
    model: String,

    #[arg(long, global = true)]
    prompt: Option<String>,

    #[arg(long, global = true)]
    temperature: Option<f32>,

    #[arg(long, global = true)]
    max_tokens: Option<usize>,

    #[arg(
        long,
        global = true,
        value_parser = clap::value_parser!(u8).range(1..=3),
        help = "Review strictness (1=high-signal, 3=deep scan)"
    )]
    strictness: Option<u8>,

    #[arg(
        long,
        global = true,
        value_delimiter = ',',
        help = "Comment types: logic,syntax,style,informational"
    )]
    comment_types: Option<Vec<String>>,

    #[arg(
        long,
        global = true,
        value_parser = clap::value_parser!(bool),
        help = "Use OpenAI Responses API (true/false)"
    )]
    openai_responses: Option<bool>,

    #[arg(long, global = true, default_value = "json")]
    output_format: OutputFormat,

    #[arg(short, long, global = true)]
    verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Force an LSP command for symbol indexing (enables LSP provider)"
    )]
    lsp_command: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    Review {
        #[arg(long)]
        diff: Option<PathBuf>,

        #[arg(long)]
        patch: bool,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Git {
        #[command(subcommand)]
        command: GitCommands,
    },
    Pr {
        #[arg(long)]
        number: Option<u32>,

        #[arg(long)]
        repo: Option<String>,

        #[arg(long)]
        post_comments: bool,

        #[arg(long)]
        summary: bool,
    },
    Compare {
        #[arg(long)]
        old_file: PathBuf,

        #[arg(long)]
        new_file: PathBuf,
    },
    #[command(about = "Enhanced code review with confidence scoring and executive summaries")]
    SmartReview {
        #[arg(long, help = "Path to diff file (reads from stdin if not provided)")]
        diff: Option<PathBuf>,

        #[arg(
            short,
            long,
            help = "Output file path (prints to stdout if not provided)"
        )]
        output: Option<PathBuf>,
    },
    #[command(about = "Generate changelog and release notes from git history")]
    Changelog {
        #[arg(long, help = "Starting tag/commit (defaults to most recent tag)")]
        from: Option<String>,

        #[arg(long, help = "Ending ref (defaults to HEAD)")]
        to: Option<String>,

        #[arg(long, help = "Generate release notes for a specific version")]
        release: Option<String>,

        #[arg(
            short,
            long,
            help = "Output file path (prints to stdout if not provided)"
        )]
        output: Option<PathBuf>,
    },
    #[command(about = "Preflight LSP setup and configuration")]
    LspCheck {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    Feedback {
        #[arg(
            long,
            value_name = "FILE",
            help = "Mark review JSON comments as accepted"
        )]
        accept: Option<PathBuf>,

        #[arg(
            long,
            value_name = "FILE",
            help = "Mark review JSON comments as rejected"
        )]
        reject: Option<PathBuf>,

        #[arg(long, help = "Override feedback file path")]
        feedback_path: Option<PathBuf>,
    },
    #[command(about = "Ask follow-up questions on generated review comments")]
    Discuss {
        #[arg(
            long,
            value_name = "FILE",
            help = "Path to review comments JSON (output-format json)"
        )]
        review: PathBuf,

        #[arg(long, help = "Comment id to discuss")]
        comment_id: Option<String>,

        #[arg(long, help = "1-based comment index in the review JSON")]
        comment_index: Option<usize>,

        #[arg(long, help = "Question for the selected comment")]
        question: Option<String>,

        #[arg(long, help = "Persist follow-up thread to this file")]
        thread: Option<PathBuf>,

        #[arg(long, help = "Interactive discussion mode")]
        interactive: bool,
    },
    #[command(about = "Evaluate review quality against fixture expectations")]
    Eval {
        #[arg(long, default_value = "eval/fixtures")]
        fixtures: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GitCommands {
    Uncommitted,
    Staged,
    Branch {
        #[arg(help = "Base branch/ref (defaults to repo default)")]
        base: Option<String>,
    },
    Suggest,
    PrTitle,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OutputFormat {
    Json,
    Patch,
    Markdown,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Load configuration from file and merge with CLI options
    let mut config = config::Config::load().unwrap_or_default();
    config.merge_with_cli(Some(cli.model.clone()), cli.prompt.clone());

    // Override with CLI temperature and max_tokens if provided
    if let Some(temp) = cli.temperature {
        config.temperature = temp;
    }
    if let Some(tokens) = cli.max_tokens {
        config.max_tokens = tokens;
    }
    if let Some(strictness) = cli.strictness {
        config.strictness = strictness;
    }
    if let Some(comment_types) = cli.comment_types {
        config.comment_types = comment_types;
    }
    if let Some(flag) = cli.openai_responses {
        config.openai_use_responses = Some(flag);
    }
    if let Some(command) = cli.lsp_command {
        config.symbol_index = true;
        config.symbol_index_provider = "lsp".to_string();
        config.symbol_index_lsp_command = Some(command);
    }
    config.normalize();

    match cli.command {
        Commands::Review {
            diff,
            patch,
            output,
        } => {
            review_command(config, diff, patch, output, cli.output_format).await?;
        }
        Commands::Check { path } => {
            check_command(path, config, cli.output_format).await?;
        }
        Commands::Git { command } => {
            git_command(command, config, cli.output_format).await?;
        }
        Commands::Pr {
            number,
            repo,
            post_comments,
            summary,
        } => {
            pr_command(
                number,
                repo,
                post_comments,
                summary,
                config,
                cli.output_format,
            )
            .await?;
        }
        Commands::Compare { old_file, new_file } => {
            compare_command(old_file, new_file, config, cli.output_format).await?;
        }
        Commands::SmartReview { diff, output } => {
            smart_review_command(config, diff, output).await?;
        }
        Commands::Changelog {
            from,
            to,
            release,
            output,
        } => {
            changelog_command(from, to, release, output).await?;
        }
        Commands::LspCheck { path } => {
            lsp_check_command(path, config).await?;
        }
        Commands::Feedback {
            accept,
            reject,
            feedback_path,
        } => {
            feedback_command(config, accept, reject, feedback_path).await?;
        }
        Commands::Discuss {
            review,
            comment_id,
            comment_index,
            question,
            thread,
            interactive,
        } => {
            discuss_command(
                config,
                review,
                comment_id,
                comment_index,
                question,
                thread,
                interactive,
            )
            .await?;
        }
        Commands::Eval { fixtures, output } => {
            eval_command(config, fixtures, output).await?;
        }
    }

    Ok(())
}

async fn review_command(
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
    let pattern_repositories = resolve_pattern_repositories(&config, &repo_root);
    let review_rules = load_review_rules(&config, &pattern_repositories, &repo_root);

    let mut plugin_manager = plugins::plugin::PluginManager::new();
    plugin_manager.load_builtin_plugins(&config.plugins).await?;
    let feedback = load_feedback_store(&config);

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
    let symbol_index = build_symbol_index(&config, &repo_root);
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

        if let Some(guidance) = build_review_guidance(&config, path_config) {
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

    let processed_comments = plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;
    let processed_comments = apply_review_filters(processed_comments, &config, &feedback);

    let effective_format = if patch { OutputFormat::Patch } else { format };
    output_comments(&processed_comments, output_path, effective_format).await?;

    Ok(())
}

async fn check_command(path: PathBuf, config: config::Config, format: OutputFormat) -> Result<()> {
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

async fn lsp_check_command(path: PathBuf, config: config::Config) -> Result<()> {
    let repo_root = core::GitIntegration::new(&path)
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or(path);

    println!("LSP health check");
    println!("repo: {}", repo_root.display());
    println!(
        "symbol_index: {}",
        if config.symbol_index {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("symbol_index_provider: {}", config.symbol_index_provider);
    if !config.symbol_index {
        println!("note: symbol_index is disabled (set symbol_index: true)");
    }
    if config.symbol_index_provider != "lsp" {
        println!("note: symbol_index_provider is not lsp (set symbol_index_provider: lsp)");
    }

    let configured_command = config.symbol_index_lsp_command.clone();
    let detected_command = if configured_command.is_none() {
        core::SymbolIndex::detect_lsp_command(
            &repo_root,
            config.symbol_index_max_files,
            &config.symbol_index_lsp_languages,
            |path| config.should_exclude(path),
        )
    } else {
        None
    };

    if let Some(command) = &configured_command {
        println!("configured LSP command: {}", command);
    }
    if let Some(command) = &detected_command {
        println!("detected LSP command: {}", command);
    }

    let effective_command = configured_command.or(detected_command);
    if let Some(command) = &effective_command {
        let available = core::SymbolIndex::lsp_command_available(command);
        println!("effective LSP command: {}", command);
        println!(
            "command available: {}",
            if available { "yes" } else { "no" }
        );
    } else {
        println!("effective LSP command: <none>");
        println!("command available: no");
    }

    let mut normalized_languages = HashMap::new();
    let mut invalid_mappings = Vec::new();
    for (ext, language) in &config.symbol_index_lsp_languages {
        let ext = ext.trim().to_ascii_lowercase();
        let language = language.trim().to_string();
        if ext.is_empty() || language.is_empty() {
            invalid_mappings.push(format!("{}:{}", ext, language));
            continue;
        }
        normalized_languages.insert(ext, language);
    }

    if normalized_languages.is_empty() {
        println!("language map: empty (set symbol_index_lsp_languages)");
    } else {
        println!("language map entries: {}", normalized_languages.len());
    }
    if !invalid_mappings.is_empty() {
        println!(
            "invalid language map entries: {}",
            invalid_mappings.join(", ")
        );
    }

    let extension_counts = core::SymbolIndex::scan_extension_counts(
        &repo_root,
        config.symbol_index_max_files,
        |path| config.should_exclude(path),
    );
    if extension_counts.is_empty() {
        println!("repo extensions: none detected (check path or excludes)");
        return Ok(());
    }

    let mut extension_list: Vec<_> = extension_counts.iter().collect();
    extension_list.sort_by(|(a_ext, a_count), (b_ext, b_count)| {
        b_count.cmp(a_count).then_with(|| a_ext.cmp(b_ext))
    });
    let top_extensions: Vec<String> = extension_list
        .iter()
        .take(10)
        .map(|(ext, count)| format!("{}({})", ext, count))
        .collect();
    println!("top extensions: {}", top_extensions.join(", "));

    let mut unmapped = Vec::new();
    for ext in extension_counts.keys() {
        if !normalized_languages.contains_key(ext) {
            unmapped.push(ext.clone());
        }
    }
    unmapped.sort();
    if !unmapped.is_empty() {
        println!("unmapped repo extensions: {}", unmapped.join(", "));
    }

    let mut unused = Vec::new();
    for ext in normalized_languages.keys() {
        if !extension_counts.contains_key(ext) {
            unused.push(ext.clone());
        }
    }
    unused.sort();
    if !unused.is_empty() {
        println!("unused language map entries: {}", unused.join(", "));
    }

    Ok(())
}

async fn git_command(
    command: GitCommands,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    let git = core::GitIntegration::new(".")?;

    let diff_content = match command {
        GitCommands::Uncommitted => {
            info!("Analyzing uncommitted changes");
            git.get_uncommitted_diff()?
        }
        GitCommands::Staged => {
            info!("Analyzing staged changes");
            git.get_staged_diff()?
        }
        GitCommands::Branch { base } => {
            let base_branch = base.unwrap_or_else(|| {
                git.get_default_branch()
                    .unwrap_or_else(|_| "main".to_string())
            });
            info!("Analyzing changes from branch: {}", base_branch);
            git.get_branch_diff(&base_branch)?
        }
        GitCommands::Suggest => {
            return suggest_commit_message(config).await;
        }
        GitCommands::PrTitle => {
            return suggest_pr_title(config).await;
        }
    };

    if diff_content.is_empty() {
        println!("No changes found");
        return Ok(());
    }

    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));
    review_diff_content_with_repo(&diff_content, config, format, &repo_root).await
}

async fn pr_command(
    number: Option<u32>,
    repo: Option<String>,
    post_comments: bool,
    summary: bool,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    use std::process::Command;

    let pr_number = if let Some(num) = number {
        num.to_string()
    } else {
        // Get current PR number
        let mut args = vec![
            "pr".to_string(),
            "view".to_string(),
            "--json".to_string(),
            "number".to_string(),
            "-q".to_string(),
            ".number".to_string(),
        ];
        if let Some(repo) = repo.as_ref() {
            args.push("--repo".to_string());
            args.push(repo.clone());
        }

        let output = Command::new("gh").args(&args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh pr view failed: {}", stderr.trim());
        }

        let pr_number = String::from_utf8(output.stdout)?.trim().to_string();
        if pr_number.is_empty() {
            anyhow::bail!("Unable to determine PR number from gh output");
        }
        pr_number
    };

    info!("Reviewing PR #{}", pr_number);

    // Get additional git context
    let git = core::GitIntegration::new(".")?;
    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));
    if let Ok(branch) = git.get_current_branch() {
        info!("Current branch: {}", branch);
    }
    if let Ok(Some(remote)) = git.get_remote_url() {
        info!("Remote URL: {}", remote);
    }

    // Get PR diff
    let mut diff_args = vec!["pr".to_string(), "diff".to_string(), pr_number.clone()];
    if let Some(repo) = repo.as_ref() {
        diff_args.push("--repo".to_string());
        diff_args.push(repo.clone());
    }
    let diff_output = Command::new("gh").args(&diff_args).output()?;
    if !diff_output.status.success() {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        anyhow::bail!("gh pr diff failed: {}", stderr.trim());
    }

    let diff_content = String::from_utf8(diff_output.stdout)?;

    if diff_content.is_empty() {
        println!("No changes in PR");
        return Ok(());
    }

    // Generate PR summary if requested
    if summary {
        let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
        let git = core::GitIntegration::new(".")?;

        let model_config = adapters::llm::ModelConfig {
            model_name: config.model.clone(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            openai_use_responses: config.openai_use_responses,
        };

        let adapter = adapters::llm::create_adapter(&model_config)?;
        let options = core::SummaryOptions {
            include_diagram: config.smart_review_diagram,
        };
        let pr_summary = core::PRSummaryGenerator::generate_summary_with_options(
            &diffs,
            &git,
            adapter.as_ref(),
            options,
        )
        .await?;

        println!("{}", pr_summary.to_markdown());
        return Ok(());
    }

    let comments = review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;

    if post_comments {
        info!("Posting {} comments to PR", comments.len());
        let metadata = fetch_pr_metadata(&pr_number, repo.as_ref())?;
        let mut inline_posted = 0usize;
        let mut fallback_posted = 0usize;

        for comment in &comments {
            let body = build_github_comment_body(comment);
            let inline_result =
                post_inline_pr_comment(&pr_number, repo.as_ref(), &metadata, comment, &body);

            if inline_result.is_ok() {
                inline_posted += 1;
                continue;
            }

            if let Err(err) = inline_result {
                warn!(
                    "Inline comment failed for {}:{} (falling back to PR comment): {}",
                    comment.file_path.display(),
                    comment.line_number,
                    err
                );
            }
            post_pr_comment(&pr_number, repo.as_ref(), &body)?;
            fallback_posted += 1;
        }
        upsert_pr_summary_comment(&pr_number, repo.as_ref(), &metadata, &comments)?;

        println!(
            "Posted {} comments to PR #{} (inline: {}, fallback: {}, summary: updated)",
            comments.len(),
            pr_number,
            inline_posted,
            fallback_posted
        );
    } else {
        output_comments(&comments, None, format).await?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GhPrMetadata {
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "baseRepository")]
    base_repository: GhBaseRepository,
}

#[derive(Debug, Deserialize)]
struct GhBaseRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

fn fetch_pr_metadata(pr_number: &str, repo: Option<&String>) -> Result<GhPrMetadata> {
    use std::process::Command;

    let mut args = vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "headRefOid,baseRepository".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view metadata failed: {}", stderr.trim());
    }

    let metadata: GhPrMetadata = serde_json::from_slice(&output.stdout)?;
    Ok(metadata)
}

fn build_github_comment_body(comment: &core::Comment) -> String {
    let mut body = format!(
        "**{:?} ({:?})**\n\n{}",
        comment.severity, comment.category, comment.content
    );
    if let Some(suggestion) = &comment.suggestion {
        body.push_str("\n\n**Suggested fix:** ");
        body.push_str(suggestion);
    }
    body.push_str(&format!(
        "\n\n_Confidence: {:.0}%_",
        comment.confidence * 100.0
    ));
    body
}

fn post_inline_pr_comment(
    pr_number: &str,
    repo: Option<&String>,
    metadata: &GhPrMetadata,
    comment: &core::Comment,
    body: &str,
) -> Result<()> {
    use std::process::Command;

    if comment.line_number == 0 {
        anyhow::bail!("line number is 0");
    }

    let endpoint = format!(
        "repos/{}/pulls/{}/comments",
        metadata.base_repository.name_with_owner, pr_number
    );
    let mut args = vec![
        "api".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        endpoint,
        "-f".to_string(),
        format!("body={}", body),
        "-f".to_string(),
        format!("commit_id={}", metadata.head_ref_oid),
        "-f".to_string(),
        format!("path={}", comment.file_path.display()),
        "-F".to_string(),
        format!("line={}", comment.line_number),
        "-f".to_string(),
        "side=RIGHT".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api inline comment failed: {}", stderr.trim());
    }

    Ok(())
}

fn post_pr_comment(pr_number: &str, repo: Option<&String>, body: &str) -> Result<()> {
    use std::process::Command;

    let mut args = vec![
        "pr".to_string(),
        "comment".to_string(),
        pr_number.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr comment failed: {}", stderr.trim());
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GhIssueComment {
    id: u64,
    body: String,
}

fn upsert_pr_summary_comment(
    pr_number: &str,
    repo: Option<&String>,
    metadata: &GhPrMetadata,
    comments: &[core::Comment],
) -> Result<()> {
    use std::process::Command;

    const SUMMARY_MARKER: &str = "<!-- diffscope:summary -->";
    let summary_body = build_pr_summary_comment_body(comments);
    let full_body = format!("{}\n\n{}", SUMMARY_MARKER, summary_body);

    let comments_endpoint = format!(
        "repos/{}/issues/{}/comments?per_page=100",
        metadata.base_repository.name_with_owner, pr_number
    );
    let mut args = vec!["api".to_string(), comments_endpoint];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api list issue comments failed: {}", stderr.trim());
    }

    let issue_comments: Vec<GhIssueComment> = serde_json::from_slice(&output.stdout)?;
    if let Some(existing) = issue_comments
        .iter()
        .find(|comment| comment.body.contains(SUMMARY_MARKER))
    {
        let patch_endpoint = format!(
            "repos/{}/issues/comments/{}",
            metadata.base_repository.name_with_owner, existing.id
        );
        let mut patch_args = vec![
            "api".to_string(),
            "-X".to_string(),
            "PATCH".to_string(),
            patch_endpoint,
            "-f".to_string(),
            format!("body={}", full_body),
        ];
        if let Some(repo) = repo {
            patch_args.push("--repo".to_string());
            patch_args.push(repo.clone());
        }

        let patch_output = Command::new("gh").args(&patch_args).output()?;
        if !patch_output.status.success() {
            let stderr = String::from_utf8_lossy(&patch_output.stderr);
            anyhow::bail!("gh api patch summary comment failed: {}", stderr.trim());
        }
        return Ok(());
    }

    post_pr_comment(pr_number, repo, &full_body)
}

fn build_pr_summary_comment_body(comments: &[core::Comment]) -> String {
    let summary = core::CommentSynthesizer::generate_summary(comments);
    let mut body = String::new();
    body.push_str("## DiffScope Review Summary\n\n");
    body.push_str(&format!("- Total issues: {}\n", summary.total_comments));
    body.push_str(&format!("- Critical issues: {}\n", summary.critical_issues));
    body.push_str(&format!("- Files reviewed: {}\n", summary.files_reviewed));
    body.push_str(&format!(
        "- Overall score: {:.1}/10\n",
        summary.overall_score
    ));

    if summary.total_comments == 0 {
        body.push_str("\nNo issues detected in this PR by DiffScope.\n");
        return body;
    }

    body.push_str("\n### Severity Breakdown\n");
    for severity in ["Error", "Warning", "Info", "Suggestion"] {
        let count = summary.by_severity.get(severity).copied().unwrap_or(0);
        body.push_str(&format!("- {}: {}\n", severity, count));
    }

    let rule_hits = summarize_rule_hits(comments, 8);
    if !rule_hits.is_empty() {
        body.push_str("\n### Rule Hits\n");
        for (rule_id, hit) in rule_hits {
            body.push_str(&format!(
                "- `{}`: {} hit(s) (E:{} W:{} I:{} S:{})\n",
                rule_id, hit.total, hit.errors, hit.warnings, hit.infos, hit.suggestions
            ));
        }
    }

    body.push_str("\n### Top Findings\n");
    for (shown, comment) in comments.iter().enumerate() {
        if shown >= 5 {
            break;
        }
        body.push_str(&format!(
            "- `{}:{}` [{:?}] {}\n",
            comment.file_path.display(),
            comment.line_number,
            comment.severity,
            comment.content
        ));
    }

    body
}

#[derive(Debug, Default, Clone, Copy)]
struct RuleHitBreakdown {
    total: usize,
    errors: usize,
    warnings: usize,
    infos: usize,
    suggestions: usize,
}

fn summarize_rule_hits(
    comments: &[core::Comment],
    max_rules: usize,
) -> Vec<(String, RuleHitBreakdown)> {
    let mut by_rule: HashMap<String, RuleHitBreakdown> = HashMap::new();
    for comment in comments {
        let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) else {
            continue;
        };
        let hit = by_rule.entry(rule_id).or_default();
        hit.total = hit.total.saturating_add(1);
        match comment.severity {
            core::comment::Severity::Error => hit.errors = hit.errors.saturating_add(1),
            core::comment::Severity::Warning => hit.warnings = hit.warnings.saturating_add(1),
            core::comment::Severity::Info => hit.infos = hit.infos.saturating_add(1),
            core::comment::Severity::Suggestion => {
                hit.suggestions = hit.suggestions.saturating_add(1);
            }
        }
    }

    let mut rows = by_rule.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .1
            .total
            .cmp(&left.1.total)
            .then_with(|| right.1.errors.cmp(&left.1.errors))
            .then_with(|| left.0.cmp(&right.0))
    });
    rows.truncate(max_rules);
    rows
}

async fn suggest_commit_message(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let diff_content = git.get_staged_diff()?;

    if diff_content.is_empty() {
        println!("No staged changes found. Stage your changes with 'git add' first.");
        return Ok(());
    }

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_commit_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(500),
    };

    let response = adapter.complete(request).await?;
    let commit_message = core::CommitPromptBuilder::extract_commit_message(&response.content);

    println!("\nSuggested commit message:");
    println!("{}", commit_message);

    if commit_message.len() > 72 {
        println!(
            "\n⚠️  Warning: Commit message exceeds 72 characters ({})",
            commit_message.len()
        );
    }

    Ok(())
}

async fn suggest_pr_title(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let base_branch = git
        .get_default_branch()
        .unwrap_or_else(|_| "main".to_string());
    let diff_content = git.get_branch_diff(&base_branch)?;

    if diff_content.is_empty() {
        println!("No changes found compared to {} branch.", base_branch);
        return Ok(());
    }

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_pr_title_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(200),
    };

    let response = adapter.complete(request).await?;

    // Extract title from response
    let title = if let Some(start) = response.content.find("<title>") {
        if let Some(end) = response.content.find("</title>") {
            response.content[start + 7..end].trim().to_string()
        } else {
            response.content.trim().to_string()
        }
    } else {
        // Fallback: take the first non-empty line
        response
            .content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    };

    println!("\nSuggested PR title:");
    println!("{}", title);

    if title.len() > 65 {
        println!(
            "\n⚠️  Warning: PR title exceeds 65 characters ({})",
            title.len()
        );
    }

    Ok(())
}

async fn compare_command(
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

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalFixture {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    diff: Option<String>,
    #[serde(default)]
    diff_file: Option<PathBuf>,
    #[serde(default)]
    repo_path: Option<PathBuf>,
    #[serde(default)]
    expect: EvalExpectations,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalExpectations {
    #[serde(default)]
    must_find: Vec<EvalPattern>,
    #[serde(default)]
    must_not_find: Vec<EvalPattern>,
    #[serde(default)]
    min_total: Option<usize>,
    #[serde(default)]
    max_total: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalPattern {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<usize>,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    rule_id: Option<String>,
    #[serde(default)]
    require_rule_id: bool,
}

#[derive(Debug, Clone, Serialize)]
struct EvalRuleMetrics {
    rule_id: String,
    expected: usize,
    predicted: usize,
    true_positives: usize,
    false_positives: usize,
    false_negatives: usize,
    precision: f32,
    recall: f32,
    f1: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
struct EvalRuleScoreSummary {
    micro_precision: f32,
    micro_recall: f32,
    micro_f1: f32,
    macro_precision: f32,
    macro_recall: f32,
    macro_f1: f32,
}

#[derive(Debug, Clone, Serialize)]
struct EvalFixtureResult {
    fixture: String,
    passed: bool,
    total_comments: usize,
    required_matches: usize,
    required_total: usize,
    rule_metrics: Vec<EvalRuleMetrics>,
    rule_summary: Option<EvalRuleScoreSummary>,
    failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EvalReport {
    fixtures_total: usize,
    fixtures_passed: usize,
    fixtures_failed: usize,
    rule_metrics: Vec<EvalRuleMetrics>,
    rule_summary: Option<EvalRuleScoreSummary>,
    results: Vec<EvalFixtureResult>,
}

async fn eval_command(
    config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
) -> Result<()> {
    let fixture_paths = collect_fixture_paths(&fixtures_dir)?;
    if fixture_paths.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let mut results = Vec::new();
    for fixture_path in fixture_paths {
        let fixture = load_eval_fixture(&fixture_path)?;
        let result = run_eval_fixture(&config, &fixture_path, fixture).await?;
        results.push(result);
    }

    let fixtures_total = results.len();
    let fixtures_passed = results.iter().filter(|result| result.passed).count();
    let fixtures_failed = fixtures_total.saturating_sub(fixtures_passed);
    let rule_metrics = aggregate_rule_metrics(&results);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
    let report = EvalReport {
        fixtures_total,
        fixtures_passed,
        fixtures_failed,
        rule_metrics,
        rule_summary,
        results,
    };

    println!(
        "Eval summary: {}/{} fixture(s) passed",
        report.fixtures_passed, report.fixtures_total
    );
    for result in &report.results {
        if result.passed {
            println!(
                "[PASS] {} ({} comments, {}/{})",
                result.fixture,
                result.total_comments,
                result.required_matches,
                result.required_total
            );
        } else {
            println!(
                "[FAIL] {} ({} comments, {}/{})",
                result.fixture,
                result.total_comments,
                result.required_matches,
                result.required_total
            );
            for failure in &result.failures {
                println!("  - {}", failure);
            }
        }
        if let Some(rule_summary) = result.rule_summary {
            println!(
                "  rule-metrics: micro P={:.0}% R={:.0}% F1={:.0}%",
                rule_summary.micro_precision * 100.0,
                rule_summary.micro_recall * 100.0,
                rule_summary.micro_f1 * 100.0
            );
        }
    }

    if let Some(rule_summary) = report.rule_summary {
        println!(
            "Rule metrics (micro): P={:.0}% R={:.0}% F1={:.0}%",
            rule_summary.micro_precision * 100.0,
            rule_summary.micro_recall * 100.0,
            rule_summary.micro_f1 * 100.0
        );
        println!(
            "Rule metrics (macro): P={:.0}% R={:.0}% F1={:.0}%",
            rule_summary.macro_precision * 100.0,
            rule_summary.macro_recall * 100.0,
            rule_summary.macro_f1 * 100.0
        );

        for metric in report.rule_metrics.iter().take(8) {
            println!(
                "  - {}: tp={} fp={} fn={} (P={:.0}% R={:.0}%)",
                metric.rule_id,
                metric.true_positives,
                metric.false_positives,
                metric.false_negatives,
                metric.precision * 100.0,
                metric.recall * 100.0
            );
        }
    }

    if let Some(path) = output_path {
        let serialized = serde_json::to_string_pretty(&report)?;
        tokio::fs::write(path, serialized).await?;
    }

    if report.fixtures_failed > 0 {
        anyhow::bail!(
            "Evaluation failed: {} fixture(s) did not meet expectations",
            report.fixtures_failed
        );
    }

    Ok(())
}

fn collect_fixture_paths(fixtures_dir: &Path) -> Result<Vec<PathBuf>> {
    if !fixtures_dir.exists() {
        anyhow::bail!("Fixtures directory not found: {}", fixtures_dir.display());
    }
    if !fixtures_dir.is_dir() {
        anyhow::bail!(
            "Fixtures path is not a directory: {}",
            fixtures_dir.display()
        );
    }

    let mut paths = Vec::new();
    let mut stack = vec![fixtures_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if matches!(extension.as_deref(), Some("json" | "yml" | "yaml")) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

fn load_eval_fixture(path: &Path) -> Result<EvalFixture> {
    let content = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Ok(serde_json::from_str(&content)?),
        _ => match serde_yaml::from_str(&content) {
            Ok(parsed) => Ok(parsed),
            Err(_) => Ok(serde_json::from_str(&content)?),
        },
    }
}

async fn run_eval_fixture(
    config: &config::Config,
    fixture_path: &Path,
    fixture: EvalFixture,
) -> Result<EvalFixtureResult> {
    let fixture_name = fixture.name.unwrap_or_else(|| {
        fixture_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("fixture")
            .to_string()
    });
    let fixture_dir = fixture_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let diff_content = match (fixture.diff, fixture.diff_file) {
        (Some(diff), _) => diff,
        (None, Some(diff_file)) => {
            let path = if diff_file.is_absolute() {
                diff_file
            } else {
                fixture_dir.join(diff_file)
            };
            std::fs::read_to_string(path)?
        }
        (None, None) => anyhow::bail!(
            "Fixture '{}' must define either diff or diff_file",
            fixture_name
        ),
    };

    let repo_path = fixture
        .repo_path
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                fixture_dir.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let comments = review_diff_content_raw(&diff_content, config.clone(), &repo_path).await?;
    let total_comments = comments.len();
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let required_total = fixture.expect.must_find.len();
    let mut used_comment_indices = HashSet::new();
    let mut matched_pairs = Vec::new();

    for (expected_idx, expected) in fixture.expect.must_find.iter().enumerate() {
        let found = comments
            .iter()
            .enumerate()
            .find(|(comment_idx, comment)| {
                !used_comment_indices.contains(comment_idx) && expected.matches(comment)
            })
            .map(|(comment_idx, _)| comment_idx);

        if let Some(comment_idx) = found {
            used_comment_indices.insert(comment_idx);
            matched_pairs.push((expected_idx, comment_idx));
            required_matches = required_matches.saturating_add(1);
        } else {
            failures.push(format!("Missing expected finding: {}", expected.describe()));
        }
    }

    for unexpected in &fixture.expect.must_not_find {
        if let Some(comment) = comments.iter().find(|comment| unexpected.matches(comment)) {
            failures.push(format!(
                "Unexpected finding matched {}:{} '{}'",
                comment.file_path.display(),
                comment.line_number,
                summarize_for_eval(&comment.content)
            ));
        }
    }

    let rule_metrics = compute_rule_metrics(&fixture.expect.must_find, &comments, &matched_pairs);
    let rule_summary = summarize_rule_metrics(&rule_metrics);

    if let Some(min_total) = fixture.expect.min_total {
        if total_comments < min_total {
            failures.push(format!(
                "Expected at least {} comments, got {}",
                min_total, total_comments
            ));
        }
    }
    if let Some(max_total) = fixture.expect.max_total {
        if total_comments > max_total {
            failures.push(format!(
                "Expected at most {} comments, got {}",
                max_total, total_comments
            ));
        }
    }

    Ok(EvalFixtureResult {
        fixture: fixture_name,
        passed: failures.is_empty(),
        total_comments,
        required_matches,
        required_total,
        rule_metrics,
        rule_summary,
        failures,
    })
}

#[derive(Debug, Default, Clone, Copy)]
struct RuleMetricCounts {
    expected: usize,
    predicted: usize,
    true_positives: usize,
}

fn compute_rule_metrics(
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();

    for pattern in expected_patterns {
        if let Some(rule_id) = pattern.normalized_rule_id() {
            counts_by_rule.entry(rule_id).or_default().expected += 1;
        }
    }

    for comment in comments {
        if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
            counts_by_rule.entry(rule_id).or_default().predicted += 1;
        }
    }

    for (expected_idx, comment_idx) in matched_pairs {
        let expected_rule = expected_patterns
            .get(*expected_idx)
            .and_then(EvalPattern::normalized_rule_id);
        let predicted_rule = comments
            .get(*comment_idx)
            .and_then(|comment| normalize_rule_id(comment.rule_id.as_deref()));
        if let (Some(expected_rule), Some(predicted_rule)) = (expected_rule, predicted_rule) {
            if expected_rule == predicted_rule {
                counts_by_rule
                    .entry(expected_rule)
                    .or_default()
                    .true_positives += 1;
            }
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

fn aggregate_rule_metrics(results: &[EvalFixtureResult]) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();
    for result in results {
        for metric in &result.rule_metrics {
            let counts = counts_by_rule.entry(metric.rule_id.clone()).or_default();
            counts.expected = counts.expected.saturating_add(metric.expected);
            counts.predicted = counts.predicted.saturating_add(metric.predicted);
            counts.true_positives = counts.true_positives.saturating_add(metric.true_positives);
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

fn build_rule_metrics_from_counts(
    counts_by_rule: &HashMap<String, RuleMetricCounts>,
) -> Vec<EvalRuleMetrics> {
    let mut metrics = Vec::new();
    for (rule_id, counts) in counts_by_rule {
        let false_positives = counts.predicted.saturating_sub(counts.true_positives);
        let false_negatives = counts.expected.saturating_sub(counts.true_positives);
        let precision = if counts.predicted > 0 {
            counts.true_positives as f32 / counts.predicted as f32
        } else {
            0.0
        };
        let recall = if counts.expected > 0 {
            counts.true_positives as f32 / counts.expected as f32
        } else {
            0.0
        };
        let f1 = harmonic_mean(precision, recall);

        metrics.push(EvalRuleMetrics {
            rule_id: rule_id.clone(),
            expected: counts.expected,
            predicted: counts.predicted,
            true_positives: counts.true_positives,
            false_positives,
            false_negatives,
            precision,
            recall,
            f1,
        });
    }

    metrics.sort_by(|left, right| {
        right
            .expected
            .cmp(&left.expected)
            .then_with(|| right.predicted.cmp(&left.predicted))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    metrics
}

fn summarize_rule_metrics(metrics: &[EvalRuleMetrics]) -> Option<EvalRuleScoreSummary> {
    if metrics.is_empty() {
        return None;
    }

    let mut tp_sum = 0usize;
    let mut predicted_sum = 0usize;
    let mut expected_sum = 0usize;
    let mut precision_sum = 0.0f32;
    let mut recall_sum = 0.0f32;
    let mut f1_sum = 0.0f32;

    for metric in metrics {
        tp_sum = tp_sum.saturating_add(metric.true_positives);
        predicted_sum = predicted_sum.saturating_add(metric.predicted);
        expected_sum = expected_sum.saturating_add(metric.expected);
        precision_sum += metric.precision;
        recall_sum += metric.recall;
        f1_sum += metric.f1;
    }

    let micro_precision = if predicted_sum > 0 {
        tp_sum as f32 / predicted_sum as f32
    } else {
        0.0
    };
    let micro_recall = if expected_sum > 0 {
        tp_sum as f32 / expected_sum as f32
    } else {
        0.0
    };
    let micro_f1 = harmonic_mean(micro_precision, micro_recall);
    let count = metrics.len() as f32;

    Some(EvalRuleScoreSummary {
        micro_precision,
        micro_recall,
        micro_f1,
        macro_precision: precision_sum / count,
        macro_recall: recall_sum / count,
        macro_f1: f1_sum / count,
    })
}

fn harmonic_mean(precision: f32, recall: f32) -> f32 {
    if precision + recall <= f32::EPSILON {
        0.0
    } else {
        (2.0 * precision * recall) / (precision + recall)
    }
}

fn normalize_rule_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

impl EvalPattern {
    fn matches(&self, comment: &core::Comment) -> bool {
        if self.is_empty() {
            return false;
        }

        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                let candidate = comment.file_path.to_string_lossy();
                if !(candidate == file || candidate.ends_with(file)) {
                    return false;
                }
            }
        }

        if let Some(line) = self.line {
            if comment.line_number != line {
                return false;
            }
        }

        if let Some(contains) = &self.contains {
            let needle = contains.trim().to_ascii_lowercase();
            if !needle.is_empty() && !comment.content.to_ascii_lowercase().contains(&needle) {
                return false;
            }
        }

        if let Some(severity) = &self.severity {
            if !format!("{:?}", comment.severity).eq_ignore_ascii_case(severity.trim()) {
                return false;
            }
        }

        if let Some(category) = &self.category {
            if !format!("{:?}", comment.category).eq_ignore_ascii_case(category.trim()) {
                return false;
            }
        }

        if let Some(rule_id) = &self.rule_id {
            if self.require_rule_id {
                let expected = rule_id.trim().to_ascii_lowercase();
                let actual = comment
                    .rule_id
                    .as_deref()
                    .map(|value| value.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                if expected != actual {
                    return false;
                }
            }
        }

        true
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                parts.push(format!("file={}", file));
            }
        }
        if let Some(line) = self.line {
            parts.push(format!("line={}", line));
        }
        if let Some(contains) = &self.contains {
            let contains = contains.trim();
            if !contains.is_empty() {
                parts.push(format!("contains='{}'", contains));
            }
        }
        if let Some(severity) = &self.severity {
            let severity = severity.trim();
            if !severity.is_empty() {
                parts.push(format!("severity={}", severity));
            }
        }
        if let Some(category) = &self.category {
            let category = category.trim();
            if !category.is_empty() {
                parts.push(format!("category={}", category));
            }
        }
        if let Some(rule_id) = &self.rule_id {
            let rule_id = rule_id.trim();
            if !rule_id.is_empty() {
                if self.require_rule_id {
                    parts.push(format!("rule_id={} (required)", rule_id));
                } else {
                    parts.push(format!("rule_id={} (label)", rule_id));
                }
            }
        }

        if parts.is_empty() {
            "empty-pattern".to_string()
        } else {
            parts.join(", ")
        }
    }

    fn is_empty(&self) -> bool {
        self.file.as_deref().map(str::trim).unwrap_or("").is_empty()
            && self.line.is_none()
            && self
                .contains
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .severity
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .category
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && (!self.require_rule_id
                || self
                    .rule_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty())
    }

    fn normalized_rule_id(&self) -> Option<String> {
        normalize_rule_id(self.rule_id.as_deref())
    }
}

fn summarize_for_eval(content: &str) -> String {
    let mut summary = content.trim().replace('\n', " ");
    if summary.len() > 120 {
        summary.truncate(117);
        summary.push_str("...");
    }
    summary
}

fn format_diff_as_unified(diff: &core::UnifiedDiff) -> String {
    let mut output = String::new();

    for hunk in &diff.hunks {
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));

        for line in &hunk.changes {
            let prefix = match line.change_type {
                core::diff_parser::ChangeType::Added => "+",
                core::diff_parser::ChangeType::Removed => "-",
                core::diff_parser::ChangeType::Context => " ",
            };
            output.push_str(&format!("{}{}\n", prefix, line.content));
        }
    }

    output
}

async fn review_diff_content(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    review_diff_content_with_repo(diff_content, config, format, Path::new(".")).await
}

async fn review_diff_content_with_repo(
    diff_content: &str,
    config: config::Config,
    format: OutputFormat,
    repo_path: &Path,
) -> Result<()> {
    let comments = review_diff_content_raw(diff_content, config, repo_path).await?;
    output_comments(&comments, None, format).await
}

async fn review_diff_content_raw(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
) -> Result<Vec<core::Comment>> {
    let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
    info!("Parsed {} file diffs", diffs.len());
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

    Ok(processed_comments)
}

fn parse_llm_response(content: &str, file_path: &Path) -> Result<Vec<core::comment::RawComment>> {
    let mut comments = Vec::new();
    static LINE_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)line\s+(\d+):\s*(.+)").unwrap());

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and common non-issue lines
        if trimmed.is_empty()
            || trimmed.starts_with("```")
            || trimmed.starts_with('#')
            || trimmed.starts_with('<')
            || trimmed.contains("Here are")
            || trimmed.contains("Here is")
            || trimmed.contains("review of")
        {
            continue;
        }

        if let Some(caps) = LINE_PATTERN.captures(line) {
            let line_number: usize = caps.get(1).unwrap().as_str().parse()?;
            let comment_text = caps.get(2).unwrap().as_str().trim();
            let (rule_id, comment_text) = extract_rule_id_from_text(comment_text);

            // Extract suggestion if present
            let (content, suggestion) = if let Some(sugg_idx) = comment_text.rfind(". Consider ") {
                (
                    comment_text[..sugg_idx + 1].to_string(),
                    Some(
                        comment_text[sugg_idx + 11..]
                            .trim_end_matches('.')
                            .to_string(),
                    ),
                )
            } else if let Some(sugg_idx) = comment_text.rfind(". Use ") {
                (
                    comment_text[..sugg_idx + 1].to_string(),
                    Some(
                        comment_text[sugg_idx + 6..]
                            .trim_end_matches('.')
                            .to_string(),
                    ),
                )
            } else {
                (comment_text.to_string(), None)
            };

            comments.push(core::comment::RawComment {
                file_path: file_path.to_path_buf(),
                line_number,
                content,
                rule_id,
                suggestion,
                severity: None,
                category: None,
                confidence: None,
                fix_effort: None,
                tags: Vec::new(),
            });
        }
    }

    Ok(comments)
}

async fn output_comments(
    comments: &[core::Comment],
    output_path: Option<PathBuf>,
    format: OutputFormat,
) -> Result<()> {
    let output = match format {
        OutputFormat::Json => serde_json::to_string_pretty(comments)?,
        OutputFormat::Patch => format_as_patch(comments),
        OutputFormat::Markdown => format_as_markdown(comments),
    };

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
    } else {
        println!("{}", output);
    }

    Ok(())
}

fn format_as_patch(comments: &[core::Comment]) -> String {
    let mut output = String::new();
    for comment in comments {
        output.push_str(&format!(
            "# {}:{} - {:?}\n# {}\n",
            comment.file_path.display(),
            comment.line_number,
            comment.severity,
            comment.content
        ));
        if let Some(suggestion) = &comment.suggestion {
            output.push_str(&format!("# Suggestion: {}\n", suggestion));
        }
    }
    output
}

fn format_as_markdown(comments: &[core::Comment]) -> String {
    let mut output = String::new();

    // Generate summary
    let summary = core::CommentSynthesizer::generate_summary(comments);

    output.push_str("# Code Review Results\n\n");
    output.push_str("## Summary\n\n");
    output.push_str(&format!(
        "📊 **Overall Score:** {:.1}/10\n",
        summary.overall_score
    ));
    output.push_str(&format!(
        "📝 **Total Issues:** {}\n",
        summary.total_comments
    ));
    output.push_str(&format!(
        "🚨 **Critical Issues:** {}\n",
        summary.critical_issues
    ));
    output.push_str(&format!(
        "📁 **Files Reviewed:** {}\n\n",
        summary.files_reviewed
    ));

    // Severity breakdown
    output.push_str("### Issues by Severity\n\n");
    let severity_order = ["Error", "Warning", "Info", "Suggestion"];
    for severity in severity_order {
        let count = summary.by_severity.get(severity).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let emoji = match severity {
            "Error" => "🔴",
            "Warning" => "🟡",
            "Info" => "🔵",
            "Suggestion" => "💡",
            _ => "⚪",
        };
        output.push_str(&format!("{} **{}:** {}\n", emoji, severity, count));
    }
    output.push('\n');

    // Category breakdown
    output.push_str("### Issues by Category\n\n");
    let category_order = [
        "Security",
        "Performance",
        "Bug",
        "Maintainability",
        "Testing",
        "Style",
        "Documentation",
        "Architecture",
        "BestPractice",
    ];
    for category in category_order {
        let count = summary.by_category.get(category).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let emoji = match category {
            "Security" => "🔒",
            "Performance" => "⚡",
            "Bug" => "🐛",
            "Style" => "🎨",
            "Documentation" => "📚",
            "Testing" => "🧪",
            "Maintainability" => "🔧",
            "Architecture" => "🏗️",
            _ => "💭",
        };
        output.push_str(&format!("{} **{}:** {}\n", emoji, category, count));
    }
    output.push('\n');

    // Recommendations
    if !summary.recommendations.is_empty() {
        output.push_str("### Recommendations\n\n");
        for rec in &summary.recommendations {
            output.push_str(&format!("- {}\n", rec));
        }
        output.push('\n');
    }

    output.push_str("---\n\n## Detailed Issues\n\n");

    // Group comments by file
    let mut comments_by_file = std::collections::HashMap::new();
    for comment in comments {
        comments_by_file
            .entry(&comment.file_path)
            .or_insert_with(Vec::new)
            .push(comment);
    }

    for (file_path, file_comments) in comments_by_file {
        output.push_str(&format!("### {}\n\n", file_path.display()));

        for comment in file_comments {
            let severity_emoji = match comment.severity {
                core::comment::Severity::Error => "🔴",
                core::comment::Severity::Warning => "🟡",
                core::comment::Severity::Info => "🔵",
                core::comment::Severity::Suggestion => "💡",
            };

            let effort_badge = match comment.fix_effort {
                core::comment::FixEffort::Low => "🟢 Quick Fix",
                core::comment::FixEffort::Medium => "🟡 Moderate",
                core::comment::FixEffort::High => "🔴 Complex",
            };

            output.push_str(&format!(
                "#### Line {} {} {:?}\n\n",
                comment.line_number, severity_emoji, comment.category
            ));

            output.push_str(&format!(
                "**Confidence:** {:.0}%\n",
                comment.confidence * 100.0
            ));
            output.push_str(&format!("**Fix Effort:** {}\n\n", effort_badge));

            output.push_str(&format!("{}\n\n", comment.content));

            if let Some(suggestion) = &comment.suggestion {
                output.push_str(&format!("💡 **Suggestion:** {}\n\n", suggestion));
            }

            if let Some(code_suggestion) = &comment.code_suggestion {
                output.push_str("**Code Suggestion:**\n");
                output.push_str(&format!("```diff\n{}\n```\n\n", code_suggestion.diff));
                output.push_str(&format!("_{}_ \n\n", code_suggestion.explanation));
            }

            if !comment.tags.is_empty() {
                output.push_str("**Tags:** ");
                for (i, tag) in comment.tags.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&format!("`{}`", tag));
                }
                output.push_str("\n\n");
            }

            output.push_str("---\n\n");
        }
    }

    output
}

async fn smart_review_command(
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
    let feedback = load_feedback_store(&config);

    let mut plugin_manager = plugins::plugin::PluginManager::new();
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
    let symbol_index = build_symbol_index(&config, &repo_root);
    let pattern_repositories = resolve_pattern_repositories(&config, &repo_root);
    let review_rules = load_review_rules(&config, &pattern_repositories, &repo_root);

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };

    let adapter = adapters::llm::create_adapter(&model_config)?;
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
                    adapter.as_ref(),
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
        match core::PRSummaryGenerator::generate_change_diagram(&diffs, adapter.as_ref()).await {
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
        inject_custom_context(&config, &context_fetcher, diff, &mut context_chunks).await?;
        inject_pattern_repository_context(
            &config,
            &pattern_repositories,
            &context_fetcher,
            diff,
            &mut context_chunks,
        )
        .await?;

        // Extract symbols and get definitions
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

        let active_rules =
            core::active_rules_for_file(&review_rules, &diff.file_path, config.max_active_rules);
        inject_rule_context(diff, &active_rules, &mut context_chunks);
        context_chunks = rank_and_trim_context_chunks(
            diff,
            context_chunks,
            config.context_max_chunks,
            config.context_budget_chars,
        );

        let guidance = build_review_guidance(&config, path_config);
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
            temperature: Some(0.2), // Lower temperature for more consistent analysis
            max_tokens: Some(4000),
        };

        let response = adapter.complete(request).await?;

        if let Ok(raw_comments) = parse_smart_review_response(&response.content, &diff.file_path) {
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

    // Generate summary and output results
    let summary = core::CommentSynthesizer::generate_summary(&processed_comments);
    let output = format_smart_review_output(
        &processed_comments,
        &summary,
        pr_summary.as_ref(),
        &walkthrough,
    );

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
    } else {
        println!("{}", output);
    }

    Ok(())
}

fn parse_smart_review_response(
    content: &str,
    file_path: &Path,
) -> Result<Vec<core::comment::RawComment>> {
    let mut comments = Vec::new();
    let mut current_comment: Option<core::comment::RawComment> = None;
    let mut section: Option<SmartSection> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(title) = trimmed.strip_prefix("ISSUE:") {
            // Save previous comment if exists
            if let Some(comment) = current_comment.take() {
                comments.push(comment);
            }

            // Start new comment
            let title = title.trim();
            current_comment = Some(core::comment::RawComment {
                file_path: file_path.to_path_buf(),
                line_number: 1,
                content: title.to_string(),
                rule_id: None,
                suggestion: None,
                severity: None,
                category: None,
                confidence: None,
                fix_effort: None,
                tags: Vec::new(),
            });
            section = None;
            continue;
        }

        let comment = match current_comment.as_mut() {
            Some(comment) => comment,
            None => continue,
        };

        if let Some(value) = trimmed.strip_prefix("LINE:") {
            if let Ok(line_num) = value.trim().parse::<usize>() {
                comment.line_number = line_num;
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("RULE:") {
            let value = value.trim();
            if value.is_empty() {
                comment.rule_id = None;
            } else {
                comment.rule_id = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("SEVERITY:") {
            comment.severity = parse_smart_severity(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("CATEGORY:") {
            comment.category = parse_smart_category(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("CONFIDENCE:") {
            comment.confidence = parse_smart_confidence(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("EFFORT:") {
            comment.fix_effort = parse_smart_effort(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("TAGS:") {
            comment.tags = parse_smart_tags(value.trim());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("DESCRIPTION:") {
            section = Some(SmartSection::Description);
            let value = value.trim();
            if !value.is_empty() {
                append_content(&mut comment.content, value);
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("SUGGESTION:") {
            section = Some(SmartSection::Suggestion);
            let value = value.trim();
            if !value.is_empty() {
                append_suggestion(&mut comment.suggestion, value);
            }
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        match section {
            Some(SmartSection::Suggestion) => append_suggestion(&mut comment.suggestion, trimmed),
            _ => append_content(&mut comment.content, trimmed),
        }
    }

    // Save last comment
    if let Some(comment) = current_comment {
        comments.push(comment);
    }

    Ok(comments)
}

#[derive(Clone, Copy)]
enum SmartSection {
    Description,
    Suggestion,
}

fn append_content(content: &mut String, value: &str) {
    if !content.is_empty() {
        content.push(' ');
    }
    content.push_str(value);
}

fn append_suggestion(suggestion: &mut Option<String>, value: &str) {
    match suggestion {
        Some(existing) => {
            if !existing.is_empty() {
                existing.push(' ');
            }
            existing.push_str(value);
        }
        None => {
            *suggestion = Some(value.to_string());
        }
    }
}

fn parse_smart_severity(value: &str) -> Option<core::comment::Severity> {
    match value.to_lowercase().as_str() {
        "critical" => Some(core::comment::Severity::Error),
        "high" => Some(core::comment::Severity::Warning),
        "medium" => Some(core::comment::Severity::Info),
        "low" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

fn parse_smart_category(value: &str) -> Option<core::comment::Category> {
    match value.to_lowercase().as_str() {
        "security" => Some(core::comment::Category::Security),
        "performance" => Some(core::comment::Category::Performance),
        "bug" => Some(core::comment::Category::Bug),
        "maintainability" => Some(core::comment::Category::Maintainability),
        "testing" => Some(core::comment::Category::Testing),
        "style" => Some(core::comment::Category::Style),
        "documentation" => Some(core::comment::Category::Documentation),
        "architecture" => Some(core::comment::Category::Architecture),
        "bestpractice" | "best_practice" | "best practice" => {
            Some(core::comment::Category::BestPractice)
        }
        _ => None,
    }
}

fn parse_smart_confidence(value: &str) -> Option<f32> {
    let trimmed = value.trim().trim_end_matches('%');
    if let Ok(percent) = trimmed.parse::<f32>() {
        Some((percent / 100.0).clamp(0.0, 1.0))
    } else {
        None
    }
}

fn parse_smart_effort(value: &str) -> Option<core::comment::FixEffort> {
    match value.to_lowercase().as_str() {
        "low" => Some(core::comment::FixEffort::Low),
        "medium" => Some(core::comment::FixEffort::Medium),
        "high" => Some(core::comment::FixEffort::High),
        _ => None,
    }
}

fn parse_smart_tags(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| tag.to_string())
        .collect()
}

fn extract_rule_id_from_text(text: &str) -> (Option<String>, String) {
    static BRACKET_RULE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\[\s*rule\s*:\s*([a-z0-9_.-]+)\s*\]").unwrap());
    static PREFIX_RULE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)^rule[\s:#-]+([a-z0-9_.-]+)\s*[-:]\s*(.+)$").unwrap());

    if let Some(caps) = BRACKET_RULE.captures(text) {
        let rule_id = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty());
        let stripped = BRACKET_RULE.replace(text, "").trim().to_string();
        return (rule_id, stripped);
    }

    if let Some(caps) = PREFIX_RULE.captures(text) {
        let rule_id = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty());
        let stripped = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| text.trim().to_string());
        return (rule_id, stripped);
    }

    (None, text.trim().to_string())
}

fn format_smart_review_output(
    comments: &[core::Comment],
    summary: &core::comment::ReviewSummary,
    pr_summary: Option<&core::pr_summary::PRSummary>,
    walkthrough: &str,
) -> String {
    let mut output = String::new();

    output.push_str("# 🤖 Smart Review Analysis Results\n\n");

    // Executive Summary
    output.push_str("## 📊 Executive Summary\n\n");
    let score_emoji = if summary.overall_score >= 8.0 {
        "🟢"
    } else if summary.overall_score >= 6.0 {
        "🟡"
    } else {
        "🔴"
    };
    output.push_str(&format!(
        "{} **Code Quality Score:** {:.1}/10\n",
        score_emoji, summary.overall_score
    ));
    output.push_str(&format!(
        "📝 **Total Issues Found:** {}\n",
        summary.total_comments
    ));
    output.push_str(&format!(
        "🚨 **Critical Issues:** {}\n",
        summary.critical_issues
    ));
    output.push_str(&format!(
        "📁 **Files Analyzed:** {}\n\n",
        summary.files_reviewed
    ));

    if let Some(pr_summary) = pr_summary {
        output.push_str(&format_pr_summary_section(pr_summary));
        output.push('\n');
    }

    if !walkthrough.trim().is_empty() {
        output.push_str(walkthrough);
        output.push('\n');
    }

    // Quick Stats
    output.push_str("### 📈 Issue Breakdown\n\n");

    output.push_str("#### By Severity\n\n");
    output.push_str("| Severity | Count |\n");
    output.push_str("|----------|-------|\n");
    let severities = ["Error", "Warning", "Info", "Suggestion"];
    for severity in severities {
        let sev_count = summary.by_severity.get(severity).unwrap_or(&0);
        output.push_str(&format!("| {} | {} |\n", severity, sev_count));
    }
    output.push('\n');

    output.push_str("#### By Category\n\n");
    output.push_str("| Category | Count |\n");
    output.push_str("|----------|-------|\n");
    let categories = [
        "Security",
        "Performance",
        "Bug",
        "Maintainability",
        "Testing",
        "Style",
        "Documentation",
        "Architecture",
        "BestPractice",
    ];
    for category in categories {
        let cat_count = summary.by_category.get(category).unwrap_or(&0);
        output.push_str(&format!("| {} | {} |\n", category, cat_count));
    }
    output.push('\n');

    // Actionable Recommendations
    if !summary.recommendations.is_empty() {
        output.push_str("### 🎯 Priority Actions\n\n");
        for (i, rec) in summary.recommendations.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, rec));
        }
        output.push('\n');
    }

    if comments.is_empty() {
        output.push_str("✅ **No issues found!** Your code looks good.\n");
        return output;
    }

    output.push_str("---\n\n## 🔍 Detailed Analysis\n\n");

    // Group by severity for better organization
    let mut critical_issues = Vec::new();
    let mut high_issues = Vec::new();
    let mut medium_issues = Vec::new();
    let mut low_issues = Vec::new();

    for comment in comments {
        match comment.severity {
            core::comment::Severity::Error => critical_issues.push(comment),
            core::comment::Severity::Warning => high_issues.push(comment),
            core::comment::Severity::Info => medium_issues.push(comment),
            core::comment::Severity::Suggestion => low_issues.push(comment),
        }
    }

    // Output each severity group
    if !critical_issues.is_empty() {
        output.push_str("### 🔴 Critical Issues (Fix Immediately)\n\n");
        for comment in critical_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !high_issues.is_empty() {
        output.push_str("### 🟡 High Priority Issues\n\n");
        for comment in high_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !medium_issues.is_empty() {
        output.push_str("### 🔵 Medium Priority Issues\n\n");
        for comment in medium_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !low_issues.is_empty() {
        output.push_str("### 💡 Suggestions & Improvements\n\n");
        for comment in low_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    output
}

fn format_detailed_comment(comment: &core::Comment) -> String {
    let mut output = String::new();

    let category_emoji = match comment.category {
        core::comment::Category::Security => "🔒",
        core::comment::Category::Performance => "⚡",
        core::comment::Category::Bug => "🐛",
        core::comment::Category::Style => "🎨",
        core::comment::Category::Documentation => "📚",
        core::comment::Category::Testing => "🧪",
        core::comment::Category::Maintainability => "🔧",
        core::comment::Category::Architecture => "🏗️",
        _ => "💭",
    };

    let effort_badge = match comment.fix_effort {
        core::comment::FixEffort::Low => "🟢 Quick Fix",
        core::comment::FixEffort::Medium => "🟡 Moderate Effort",
        core::comment::FixEffort::High => "🔴 Significant Effort",
    };

    output.push_str(&format!(
        "#### {} **{}:{}** - {} {:?}\n\n",
        category_emoji,
        comment.file_path.display(),
        comment.line_number,
        effort_badge,
        comment.category
    ));

    if comment.tags.is_empty() {
        output.push_str(&format!(
            "**Confidence:** {:.0}%\n\n",
            comment.confidence * 100.0
        ));
    } else {
        output.push_str(&format!(
            "**Confidence:** {:.0}% | **Tags:** ",
            comment.confidence * 100.0
        ));
        for (i, tag) in comment.tags.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&format!("`{}`", tag));
        }
        output.push_str("\n\n");
    }

    output.push_str(&format!("{}\n\n", comment.content));

    if let Some(suggestion) = &comment.suggestion {
        output.push_str(&format!("**💡 Recommended Fix:**\n{}\n\n", suggestion));
    }

    if let Some(code_suggestion) = &comment.code_suggestion {
        output.push_str("**🔧 Code Example:**\n");
        output.push_str(&format!("```diff\n{}\n```\n", code_suggestion.diff));
        output.push_str(&format!("_{}_\n\n", code_suggestion.explanation));
    }

    output.push_str("---\n\n");
    output
}

async fn changelog_command(
    from: Option<String>,
    to: Option<String>,
    release: Option<String>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!("Generating changelog/release notes");

    let generator = core::ChangelogGenerator::new(".")?;

    let output = if let Some(version) = release {
        // Generate release notes
        info!("Generating release notes for version {}", version);
        generator.generate_release_notes(&version, from.as_deref())?
    } else {
        // Generate changelog
        let to_ref = to.as_deref().unwrap_or("HEAD");
        info!("Generating changelog from {:?} to {}", from, to_ref);
        generator.generate_changelog(from.as_deref(), to_ref)?
    };

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
        info!("Changelog written to file");
    } else {
        println!("{}", output);
    }

    Ok(())
}

async fn feedback_command(
    config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<()> {
    let (action, input_path) = match (accept, reject) {
        (Some(path), None) => ("accept", path),
        (None, Some(path)) => ("reject", path),
        _ => {
            anyhow::bail!("Specify exactly one of --accept or --reject");
        }
    };

    let feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    let content = tokio::fs::read_to_string(&input_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let mut store = load_feedback_store_from_path(&feedback_path);
    let mut updated = 0usize;

    if action == "accept" {
        for comment in &comments {
            if store.accept.insert(comment.id.clone()) {
                updated += 1;
            }
            store.suppress.remove(&comment.id);
            let key = classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.accepted = stats.accepted.saturating_add(1);
        }
    } else {
        for comment in &comments {
            if store.suppress.insert(comment.id.clone()) {
                updated += 1;
            }
            store.accept.remove(&comment.id);
            let key = classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.rejected = stats.rejected.saturating_add(1);
        }
    }

    save_feedback_store(&feedback_path, &store)?;
    println!(
        "Updated feedback store at {} ({} {} comment(s))",
        feedback_path.display(),
        updated,
        action
    );

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DiscussionTurn {
    role: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DiscussionThread {
    comment_id: String,
    turns: Vec<DiscussionTurn>,
}

async fn discuss_command(
    config: config::Config,
    review_path: PathBuf,
    comment_id: Option<String>,
    comment_index: Option<usize>,
    question: Option<String>,
    thread_path: Option<PathBuf>,
    interactive: bool,
) -> Result<()> {
    let content = tokio::fs::read_to_string(&review_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;
    if comments.is_empty() {
        anyhow::bail!("No comments found in {}", review_path.display());
    }

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let selected = select_discussion_comment(&comments, comment_id, comment_index)?;
    let mut thread = load_discussion_thread(thread_path.as_deref(), &selected.id);

    let model_config = adapters::llm::ModelConfig {
        model_name: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        openai_use_responses: config.openai_use_responses,
    };
    let adapter = adapters::llm::create_adapter(&model_config)?;

    let mut next_question = question;
    if next_question.is_none() && !interactive {
        anyhow::bail!("Provide --question or use --interactive");
    }

    loop {
        let current_question = if let Some(question) = next_question.take() {
            question
        } else if interactive {
            match read_follow_up_question()? {
                Some(question) => question,
                None => break,
            }
        } else {
            break;
        };

        let answer =
            answer_discussion_question(adapter.as_ref(), &selected, &thread, &current_question)
                .await?;

        println!("{}", answer.trim());

        thread.turns.push(DiscussionTurn {
            role: "user".to_string(),
            message: current_question,
        });
        thread.turns.push(DiscussionTurn {
            role: "assistant".to_string(),
            message: answer,
        });

        if let Some(path) = &thread_path {
            save_discussion_thread(path, &thread)?;
        }

        if !interactive {
            break;
        }
    }

    Ok(())
}

fn select_discussion_comment(
    comments: &[core::Comment],
    comment_id: Option<String>,
    comment_index: Option<usize>,
) -> Result<core::Comment> {
    if comment_id.is_some() && comment_index.is_some() {
        anyhow::bail!("Specify only one of --comment-id or --comment-index");
    }

    if let Some(id) = comment_id {
        let selected = comments
            .iter()
            .find(|comment| comment.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment id not found: {}", id))?;
        return Ok(selected);
    }

    if let Some(index) = comment_index {
        if index == 0 {
            anyhow::bail!("comment-index is 1-based");
        }
        let selected = comments
            .get(index - 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment index out of range: {}", index))?;
        return Ok(selected);
    }

    Ok(comments[0].clone())
}

fn load_discussion_thread(path: Option<&Path>, comment_id: &str) -> DiscussionThread {
    let Some(path) = path else {
        return DiscussionThread {
            comment_id: comment_id.to_string(),
            turns: Vec::new(),
        };
    };

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            return DiscussionThread {
                comment_id: comment_id.to_string(),
                turns: Vec::new(),
            };
        }
    };

    let parsed: DiscussionThread = serde_json::from_str(&content).unwrap_or_default();
    if parsed.comment_id == comment_id {
        parsed
    } else {
        DiscussionThread {
            comment_id: comment_id.to_string(),
            turns: Vec::new(),
        }
    }
}

fn save_discussion_thread(path: &Path, thread: &DiscussionThread) -> Result<()> {
    let content = serde_json::to_string_pretty(thread)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn read_follow_up_question() -> Result<Option<String>> {
    use std::io::Write;

    print!("question> ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("exit") {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

async fn answer_discussion_question(
    adapter: &dyn adapters::llm::LLMAdapter,
    comment: &core::Comment,
    thread: &DiscussionThread,
    question: &str,
) -> Result<String> {
    let mut history = String::new();
    for turn in thread.turns.iter().rev().take(8).rev() {
        history.push_str(&format!("{}: {}\n", turn.role, turn.message));
    }

    let mut prompt = String::new();
    prompt.push_str("Review comment context:\n");
    prompt.push_str(&format!(
        "- id: {}\n- file: {}\n- line: {}\n- severity: {:?}\n- category: {:?}\n- confidence: {:.0}%\n- comment: {}\n",
        comment.id,
        comment.file_path.display(),
        comment.line_number,
        comment.severity,
        comment.category,
        comment.confidence * 100.0,
        comment.content
    ));
    if let Some(suggestion) = &comment.suggestion {
        prompt.push_str(&format!("- suggested fix: {}\n", suggestion));
    }

    if !history.trim().is_empty() {
        prompt.push_str("\nPrevious follow-up thread:\n");
        prompt.push_str(&history);
    }

    prompt.push_str(&format!("\nNew question:\n{}\n", question));

    let request = adapters::llm::LLMRequest {
        system_prompt: "You are an expert reviewer assisting with follow-up questions on a specific code review comment. Answer directly, cite tradeoffs, and suggest concrete next steps. If the comment appears weak, say so and explain why.".to_string(),
        user_prompt: prompt,
        temperature: Some(0.2),
        max_tokens: Some(1200),
    };

    let response = adapter.complete(request).await?;
    Ok(response.content)
}

fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
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

fn filter_comments_for_diff(
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

fn build_review_guidance(
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

async fn inject_custom_context(
    config: &config::Config,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    for entry in config.matching_custom_context(&diff.file_path) {
        if !entry.notes.is_empty() {
            context_chunks.push(core::LLMContextChunk {
                content: format!("Custom context notes:\n{}", entry.notes.join("\n")),
                context_type: core::ContextType::Documentation,
                file_path: diff.file_path.clone(),
                line_range: None,
            });
        }

        if !entry.files.is_empty() {
            let extra_chunks = context_fetcher
                .fetch_additional_context(&entry.files)
                .await?;
            context_chunks.extend(extra_chunks);
        }
    }

    Ok(())
}

type PatternRepositoryMap = HashMap<String, PathBuf>;

fn resolve_pattern_repositories(config: &config::Config, repo_root: &Path) -> PatternRepositoryMap {
    let mut resolved = HashMap::new();
    if config.pattern_repositories.is_empty() {
        return resolved;
    }

    for repo in &config.pattern_repositories {
        if resolved.contains_key(&repo.source) {
            continue;
        }

        let source_path = Path::new(&repo.source);
        if source_path.is_absolute() && source_path.is_dir() {
            if let Ok(path) = source_path.canonicalize() {
                resolved.insert(repo.source.clone(), path);
            }
            continue;
        }

        let repo_relative = repo_root.join(&repo.source);
        if repo_relative.is_dir() {
            if let Ok(path) = repo_relative.canonicalize() {
                resolved.insert(repo.source.clone(), path);
            }
            continue;
        }

        if is_git_source(&repo.source) {
            if let Some(path) = prepare_pattern_repository_checkout(&repo.source) {
                resolved.insert(repo.source.clone(), path);
                continue;
            }
        }

        warn!(
            "Skipping pattern repository '{}' (not a readable local path or cloneable git source)",
            repo.source
        );
    }

    resolved
}

fn is_git_source(source: &str) -> bool {
    source.contains("://") || source.starts_with("git@") || source.ends_with(".git")
}

fn prepare_pattern_repository_checkout(source: &str) -> Option<PathBuf> {
    use std::process::Command;

    let home_dir = dirs::home_dir()?;
    let cache_root = home_dir.join(".diffscope").join("pattern_repositories");
    if std::fs::create_dir_all(&cache_root).is_err() {
        return None;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    let repo_dir = cache_root.join(format!("{:x}", hasher.finish()));

    if repo_dir.is_dir() {
        let pull_result = Command::new("git")
            .arg("-C")
            .arg(&repo_dir)
            .arg("pull")
            .arg("--ff-only")
            .output();
        if let Err(err) = pull_result {
            warn!(
                "Unable to update cached pattern repository {}: {}",
                source, err
            );
        }
    } else {
        let clone_result = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(source)
            .arg(&repo_dir)
            .output();
        match clone_result {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    "Failed to clone pattern repository {}: {}",
                    source,
                    stderr.trim()
                );
                return None;
            }
            Err(err) => {
                warn!("Failed to clone pattern repository {}: {}", source, err);
                return None;
            }
        }
    }

    if repo_dir.is_dir() {
        Some(repo_dir)
    } else {
        None
    }
}

async fn inject_pattern_repository_context(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let mut sources_seen = HashSet::new();
    for repo in config.matching_pattern_repositories(&diff.file_path) {
        if !sources_seen.insert(repo.source.clone()) {
            continue;
        }

        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let mut chunks = context_fetcher
            .fetch_additional_context_from_base(
                base_path,
                &repo.include_patterns,
                repo.max_files,
                repo.max_lines,
            )
            .await?;

        if chunks.is_empty() {
            continue;
        }

        context_chunks.push(core::LLMContextChunk {
            content: format!("Pattern repository context source: {}", repo.source),
            context_type: core::ContextType::Documentation,
            file_path: diff.file_path.clone(),
            line_range: None,
        });

        for chunk in &mut chunks {
            chunk.content = format!("[Pattern repository: {}]\n{}", repo.source, chunk.content);
        }
        context_chunks.extend(chunks);
    }

    Ok(())
}

fn load_review_rules(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    repo_root: &Path,
) -> Vec<core::ReviewRule> {
    let mut rules = Vec::new();
    let local_patterns = if config.rules_files.is_empty() {
        vec![
            ".diffscope-rules.yml".to_string(),
            ".diffscope-rules.yaml".to_string(),
            ".diffscope-rules.json".to_string(),
            "rules/**/*.yml".to_string(),
            "rules/**/*.yaml".to_string(),
            "rules/**/*.json".to_string(),
        ]
    } else {
        config.rules_files.clone()
    };

    let local_max_rules = config.max_active_rules.saturating_mul(8).max(64);
    match core::load_rules_from_patterns(repo_root, &local_patterns, "repository", local_max_rules)
    {
        Ok(mut loaded) => rules.append(&mut loaded),
        Err(err) => warn!("Failed to load repository rules: {}", err),
    }

    for repo in &config.pattern_repositories {
        if repo.rule_patterns.is_empty() {
            continue;
        }
        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let max_rules = repo.max_rules.max(config.max_active_rules);
        match core::load_rules_from_patterns(
            base_path,
            &repo.rule_patterns,
            &repo.source,
            max_rules,
        ) {
            Ok(mut loaded) => rules.append(&mut loaded),
            Err(err) => warn!(
                "Failed to load pattern repository rules from '{}': {}",
                repo.source, err
            ),
        }
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for rule in rules {
        let key = rule.id.trim().to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        unique.push(rule);
    }

    if !unique.is_empty() {
        info!("Loaded {} review rule(s)", unique.len());
    }
    unique
}

fn inject_rule_context(
    diff: &core::UnifiedDiff,
    active_rules: &[core::ReviewRule],
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    if active_rules.is_empty() {
        return;
    }

    let mut lines = Vec::new();
    lines.push(
        "Active review rules. If a finding maps to a rule, include `RULE: <id>` in the issue."
            .to_string(),
    );

    for rule in active_rules {
        let mut attrs = Vec::new();
        if let Some(scope) = &rule.scope {
            attrs.push(format!("scope={}", scope));
        }
        if let Some(severity) = &rule.severity {
            attrs.push(format!("severity={}", severity));
        }
        if let Some(category) = &rule.category {
            attrs.push(format!("category={}", category));
        }
        if !rule.tags.is_empty() {
            attrs.push(format!("tags={}", rule.tags.join("|")));
        }

        if attrs.is_empty() {
            lines.push(format!("- {}: {}", rule.id, rule.description));
        } else {
            lines.push(format!(
                "- {}: {} ({})",
                rule.id,
                rule.description,
                attrs.join(", ")
            ));
        }
    }

    context_chunks.push(core::LLMContextChunk {
        content: lines.join("\n"),
        context_type: core::ContextType::Documentation,
        file_path: diff.file_path.clone(),
        line_range: None,
    });
}

fn rank_and_trim_context_chunks(
    diff: &core::UnifiedDiff,
    chunks: Vec<core::LLMContextChunk>,
    max_chunks: usize,
    max_chars: usize,
) -> Vec<core::LLMContextChunk> {
    if chunks.is_empty() {
        return chunks;
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for chunk in chunks {
        let key = format!(
            "{}|{:?}|{:?}|{}",
            chunk.file_path.display(),
            chunk.context_type,
            chunk.line_range,
            chunk.content
        );
        if seen.insert(key) {
            deduped.push(chunk);
        }
    }

    let changed_ranges: Vec<(usize, usize)> = diff
        .hunks
        .iter()
        .map(|hunk| {
            (
                hunk.new_start.max(1),
                hunk.new_start
                    .saturating_add(hunk.new_lines.saturating_sub(1))
                    .max(hunk.new_start.max(1)),
            )
        })
        .collect();

    let mut scored: Vec<(i32, usize, core::LLMContextChunk)> = deduped
        .into_iter()
        .map(|chunk| {
            let mut score = match chunk.context_type {
                core::ContextType::FileContent => 130,
                core::ContextType::Definition => 100,
                core::ContextType::Reference => 80,
                core::ContextType::Documentation => 60,
            };

            if chunk.file_path == diff.file_path {
                score += 90;
            }

            if let Some(range) = chunk.line_range {
                if changed_ranges
                    .iter()
                    .any(|candidate| ranges_overlap(*candidate, range))
                {
                    score += 70;
                } else if chunk.file_path == diff.file_path {
                    score += 20;
                }
            }

            if chunk.content.starts_with("Active review rules.") {
                score += 120;
            } else if chunk
                .content
                .starts_with("Pattern repository context source:")
            {
                score += 30;
            } else if chunk.content.starts_with("[Pattern repository:") {
                score += 25;
            }

            if chunk.content.len() > 4000 {
                score -= 10;
            }

            (score, chunk.content.len(), chunk)
        })
        .collect();

    scored.sort_by_key(|(score, len, _)| (Reverse(*score), *len));

    let max_chunks = if max_chunks == 0 {
        usize::MAX
    } else {
        max_chunks
    };
    let max_chars = if max_chars == 0 {
        usize::MAX
    } else {
        max_chars
    };

    let mut kept = Vec::new();
    let mut used_chars = 0usize;

    for (_, _, chunk) in scored {
        if kept.len() >= max_chunks {
            break;
        }

        let chunk_len = chunk.content.len();
        if used_chars.saturating_add(chunk_len) > max_chars {
            continue;
        }

        used_chars = used_chars.saturating_add(chunk_len);
        kept.push(chunk);
    }

    if kept.is_empty() {
        return Vec::new();
    }

    kept
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}

fn apply_rule_overrides(
    mut comments: Vec<core::Comment>,
    active_rules: &[core::ReviewRule],
) -> Vec<core::Comment> {
    if comments.is_empty() || active_rules.is_empty() {
        return comments;
    }

    let mut by_id = HashMap::new();
    for rule in active_rules {
        by_id.insert(rule.id.to_ascii_lowercase(), rule);
    }

    for comment in &mut comments {
        let Some(rule_id) = comment.rule_id.clone() else {
            continue;
        };
        let key = rule_id.trim().to_ascii_lowercase();
        let Some(rule) = by_id.get(&key) else {
            continue;
        };

        comment.rule_id = Some(rule.id.clone());
        if let Some(severity) = rule
            .severity
            .as_deref()
            .and_then(parse_rule_severity_override)
        {
            comment.severity = severity;
        }
        if let Some(category) = rule
            .category
            .as_deref()
            .and_then(parse_rule_category_override)
        {
            comment.category = category;
        }

        let marker = format!("rule:{}", rule.id);
        if !comment.tags.iter().any(|tag| tag == &marker) {
            comment.tags.push(marker);
        }
        for tag in &rule.tags {
            if !comment.tags.iter().any(|existing| existing == tag) {
                comment.tags.push(tag.clone());
            }
        }
        comment.confidence = comment.confidence.max(0.8);
    }

    comments
}

fn parse_rule_severity_override(value: &str) -> Option<core::comment::Severity> {
    match value.trim().to_ascii_lowercase().as_str() {
        "critical" | "error" => Some(core::comment::Severity::Error),
        "high" | "warning" | "warn" => Some(core::comment::Severity::Warning),
        "medium" | "info" | "informational" => Some(core::comment::Severity::Info),
        "low" | "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

fn parse_rule_category_override(value: &str) -> Option<core::comment::Category> {
    parse_smart_category(value)
}

fn build_change_walkthrough(diffs: &[core::UnifiedDiff]) -> String {
    let mut entries = Vec::new();
    let mut truncated = false;
    let max_entries = 50usize;

    for diff in diffs {
        if diff.is_binary {
            continue;
        }

        let mut added = 0usize;
        let mut removed = 0usize;
        for hunk in &diff.hunks {
            for change in &hunk.changes {
                match change.change_type {
                    core::diff_parser::ChangeType::Added => added += 1,
                    core::diff_parser::ChangeType::Removed => removed += 1,
                    _ => {}
                }
            }
        }

        let status = if diff.is_deleted {
            "deleted"
        } else if diff.is_new {
            "new"
        } else {
            "modified"
        };

        entries.push(format!(
            "- `{}` ({}; +{}, -{})",
            diff.file_path.display(),
            status,
            added,
            removed
        ));

        if entries.len() >= max_entries {
            truncated = true;
            break;
        }
    }

    if entries.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("## 🧭 Change Walkthrough\n\n");
    output.push_str(&entries.join("\n"));
    output.push('\n');
    if truncated {
        output.push_str("\n...truncated (too many files)\n");
    }

    output
}

fn build_symbol_index(config: &config::Config, repo_root: &Path) -> Option<core::SymbolIndex> {
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

fn format_pr_summary_section(summary: &core::pr_summary::PRSummary) -> String {
    let mut output = String::new();
    output.push_str("## 🧾 PR Summary\n\n");
    output.push_str(&format!(
        "**{}** ({:?})\n\n",
        summary.title, summary.change_type
    ));

    if !summary.description.is_empty() {
        output.push_str(&format!("{}\n\n", summary.description));
    }

    if !summary.key_changes.is_empty() {
        output.push_str("### Key Changes\n\n");
        for change in &summary.key_changes {
            output.push_str(&format!("- {}\n", change));
        }
        output.push('\n');
    }

    if let Some(breaking) = &summary.breaking_changes {
        output.push_str("### Breaking Changes\n\n");
        output.push_str(&format!("{}\n\n", breaking));
    }

    if !summary.testing_notes.is_empty() {
        output.push_str("### Testing Notes\n\n");
        output.push_str(&format!("{}\n\n", summary.testing_notes));
    }

    if let Some(diagram) = &summary.visual_diff {
        if !diagram.trim().is_empty() {
            output.push_str("### Diagram\n\n");
            output.push_str("```mermaid\n");
            output.push_str(diagram.trim());
            output.push_str("\n```\n\n");
        }
    }

    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ReviewCommentType {
    Logic,
    Syntax,
    Style,
    Informational,
}

impl ReviewCommentType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Logic => "logic",
            Self::Syntax => "syntax",
            Self::Style => "style",
            Self::Informational => "informational",
        }
    }
}

fn classify_comment_type(comment: &core::Comment) -> ReviewCommentType {
    if matches!(comment.category, core::comment::Category::Style) {
        return ReviewCommentType::Style;
    }

    if matches!(
        comment.category,
        core::comment::Category::Documentation | core::comment::Category::BestPractice
    ) {
        return ReviewCommentType::Informational;
    }

    let content = comment.content.to_lowercase();
    if content.contains("syntax")
        || content.contains("parse error")
        || content.contains("compilation")
        || content.contains("compile")
        || content.contains("token")
    {
        return ReviewCommentType::Syntax;
    }

    ReviewCommentType::Logic
}

fn apply_comment_type_filter(
    comments: Vec<core::Comment>,
    enabled_types: &[String],
) -> Vec<core::Comment> {
    if enabled_types.is_empty() {
        return comments;
    }

    let enabled: HashSet<&str> = enabled_types.iter().map(String::as_str).collect();
    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        let comment_type = classify_comment_type(&comment);
        if enabled.contains(comment_type.as_str()) {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) due to comment type filters [{}]",
            dropped,
            enabled_types.join(", ")
        );
    }

    kept
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FeedbackTypeStats {
    #[serde(default)]
    accepted: usize,
    #[serde(default)]
    rejected: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FeedbackStore {
    #[serde(default)]
    suppress: HashSet<String>,
    #[serde(default)]
    accept: HashSet<String>,
    #[serde(default)]
    by_comment_type: HashMap<String, FeedbackTypeStats>,
}

fn load_feedback_store_from_path(path: &Path) -> FeedbackStore {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => FeedbackStore::default(),
    }
}

fn load_feedback_store(config: &config::Config) -> FeedbackStore {
    load_feedback_store_from_path(&config.feedback_path)
}

fn save_feedback_store(path: &Path, store: &FeedbackStore) -> Result<()> {
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn apply_review_filters(
    comments: Vec<core::Comment>,
    config: &config::Config,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    let comments = apply_confidence_threshold(comments, config.effective_min_confidence());
    let comments = apply_comment_type_filter(comments, &config.comment_types);
    apply_feedback_suppression(comments, feedback)
}

fn should_adaptively_suppress(comment: &core::Comment, feedback: &FeedbackStore) -> bool {
    if matches!(
        comment.severity,
        core::comment::Severity::Error | core::comment::Severity::Warning
    ) {
        return false;
    }

    let key = classify_comment_type(comment).as_str();
    let stats = match feedback.by_comment_type.get(key) {
        Some(stats) => stats,
        None => return false,
    };

    stats.rejected >= 3 && stats.rejected >= stats.accepted.saturating_add(2)
}

fn apply_feedback_suppression(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    if feedback.suppress.is_empty() && feedback.by_comment_type.is_empty() {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);
    let mut explicit_dropped = 0usize;
    let mut adaptive_dropped = 0usize;

    for comment in comments {
        if feedback.suppress.contains(&comment.id) {
            explicit_dropped += 1;
            continue;
        }
        if should_adaptively_suppress(&comment, feedback) {
            adaptive_dropped += 1;
            continue;
        }
        kept.push(comment);
    }

    if explicit_dropped > 0 {
        info!(
            "Dropped {} comment(s) due to explicit feedback suppression rules",
            explicit_dropped
        );
    }
    if adaptive_dropped > 0 {
        info!(
            "Dropped {} low-priority comment(s) due to learned feedback preferences",
            adaptive_dropped
        );
    }

    kept
}

fn apply_confidence_threshold(
    comments: Vec<core::Comment>,
    min_confidence: f32,
) -> Vec<core::Comment> {
    if min_confidence <= 0.0 {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        if comment.confidence >= min_confidence {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) below confidence threshold {}",
            dropped, min_confidence
        );
    }

    kept
}

fn is_line_in_diff(diff: &core::UnifiedDiff, line_number: usize) -> bool {
    if line_number == 0 {
        return false;
    }
    diff.hunks.iter().any(|hunk| {
        hunk.changes
            .iter()
            .any(|line| line.new_line_no == Some(line_number))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_comment(
        id: &str,
        category: core::comment::Category,
        severity: core::comment::Severity,
        confidence: f32,
    ) -> core::Comment {
        core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test comment".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: core::comment::FixEffort::Low,
        }
    }

    #[test]
    fn parse_smart_review_response_parses_fields() {
        let input = r#"
ISSUE: Missing auth check
LINE: 42
RULE: sec.auth.guard
SEVERITY: CRITICAL
CATEGORY: Security
CONFIDENCE: 85%
EFFORT: High

DESCRIPTION:
Authentication is missing.

SUGGESTION:
Add a guard.

TAGS: auth, security
"#;
        let file_path = PathBuf::from("src/lib.rs");
        let comments = parse_smart_review_response(input, &file_path).unwrap();
        assert_eq!(comments.len(), 1);

        let comment = &comments[0];
        assert_eq!(comment.line_number, 42);
        assert_eq!(comment.rule_id.as_deref(), Some("sec.auth.guard"));
        assert_eq!(comment.severity, Some(core::comment::Severity::Error));
        assert_eq!(comment.category, Some(core::comment::Category::Security));
        assert!(comment.content.contains("Missing auth check"));
        assert!(comment.content.contains("Authentication is missing."));
        assert_eq!(comment.suggestion.as_deref(), Some("Add a guard."));
        assert_eq!(
            comment.tags,
            vec!["auth".to_string(), "security".to_string()]
        );

        let confidence = comment.confidence.unwrap_or(0.0);
        assert!((confidence - 0.85).abs() < 0.0001);
        assert_eq!(comment.fix_effort, Some(core::comment::FixEffort::High));
    }

    #[test]
    fn comment_type_filter_keeps_only_enabled_types() {
        let comments = vec![
            build_comment(
                "logic",
                core::comment::Category::Bug,
                core::comment::Severity::Info,
                0.9,
            ),
            build_comment(
                "style",
                core::comment::Category::Style,
                core::comment::Severity::Suggestion,
                0.9,
            ),
        ];

        let enabled = vec!["logic".to_string()];
        let filtered = apply_comment_type_filter(comments, &enabled);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "logic");
    }

    #[test]
    fn adaptive_feedback_suppresses_low_priority_comment_types() {
        let feedback = FeedbackStore {
            suppress: HashSet::new(),
            accept: HashSet::new(),
            by_comment_type: HashMap::from([(
                "style".to_string(),
                FeedbackTypeStats {
                    accepted: 0,
                    rejected: 3,
                },
            )]),
        };

        let comments = vec![
            build_comment(
                "style-low",
                core::comment::Category::Style,
                core::comment::Severity::Suggestion,
                0.95,
            ),
            build_comment(
                "style-high",
                core::comment::Category::Style,
                core::comment::Severity::Error,
                0.95,
            ),
        ];

        let filtered = apply_feedback_suppression(comments, &feedback);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "style-high");
    }

    #[test]
    fn strictness_applies_minimum_confidence_floor() {
        let config = config::Config::default();
        let feedback = FeedbackStore::default();
        let comments = vec![build_comment(
            "low-confidence",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        )];

        let filtered = apply_review_filters(comments, &config, &feedback);
        assert!(filtered.is_empty());
    }

    #[test]
    fn select_discussion_comment_uses_index() {
        let comments = vec![
            build_comment(
                "c1",
                core::comment::Category::Bug,
                core::comment::Severity::Info,
                0.9,
            ),
            build_comment(
                "c2",
                core::comment::Category::Style,
                core::comment::Severity::Suggestion,
                0.8,
            ),
        ];

        let selected = select_discussion_comment(&comments, None, Some(2)).unwrap();
        assert_eq!(selected.id, "c2");
    }

    #[test]
    fn eval_pattern_rule_id_label_is_non_blocking_by_default() {
        let pattern = EvalPattern {
            file: Some("src/lib.rs".to_string()),
            line: None,
            contains: Some("test".to_string()),
            severity: None,
            category: None,
            rule_id: Some("sec.example".to_string()),
            require_rule_id: false,
        };
        let comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn eval_pattern_rule_id_can_be_required() {
        let pattern = EvalPattern {
            file: Some("src/lib.rs".to_string()),
            line: None,
            contains: Some("test".to_string()),
            severity: None,
            category: None,
            rule_id: Some("sec.example".to_string()),
            require_rule_id: true,
        };
        let comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );

        assert!(!pattern.matches(&comment));
    }

    #[test]
    fn compute_rule_metrics_tracks_tp_fp_fn() {
        let expected = vec![
            EvalPattern {
                file: Some("src/lib.rs".to_string()),
                line: None,
                contains: Some("test".to_string()),
                severity: None,
                category: None,
                rule_id: Some("rule.alpha".to_string()),
                require_rule_id: false,
            },
            EvalPattern {
                file: Some("src/lib.rs".to_string()),
                line: None,
                contains: Some("test".to_string()),
                severity: None,
                category: None,
                rule_id: Some("rule.beta".to_string()),
                require_rule_id: false,
            },
        ];
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let comments = vec![c1, c2];
        let matched_pairs = vec![(0usize, 0usize)];

        let metrics = compute_rule_metrics(&expected, &comments, &matched_pairs);
        let alpha = metrics.iter().find(|metric| metric.rule_id == "rule.alpha");
        let beta = metrics.iter().find(|metric| metric.rule_id == "rule.beta");

        let alpha = alpha.expect("expected alpha metrics");
        assert_eq!(alpha.expected, 1);
        assert_eq!(alpha.predicted, 2);
        assert_eq!(alpha.true_positives, 1);
        assert_eq!(alpha.false_positives, 1);
        assert_eq!(alpha.false_negatives, 0);

        let beta = beta.expect("expected beta metrics");
        assert_eq!(beta.expected, 1);
        assert_eq!(beta.predicted, 0);
        assert_eq!(beta.true_positives, 0);
        assert_eq!(beta.false_positives, 0);
        assert_eq!(beta.false_negatives, 1);
    }

    #[test]
    fn summarize_rule_hits_orders_by_volume() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        c3.rule_id = Some("rule.beta".to_string());

        let hits = summarize_rule_hits(&[c1, c2, c3], 8);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "rule.alpha");
        assert_eq!(hits[0].1.total, 2);
        assert_eq!(hits[1].0, "rule.beta");
    }
}
