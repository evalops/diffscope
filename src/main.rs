mod adapters;
mod commands;
mod config;
mod core;
mod output;
mod parsing;
mod plugins;
mod review;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use commands::{EvalRunOptions, GitCommands};
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "diffscope")]
#[command(about = "A composable code review engine with smart analysis and professional reporting", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, default_value = "anthropic/claude-sonnet-4.6")]
    model: String,

    #[arg(long, global = true, help = "LLM API base URL (e.g. http://localhost:11434)")]
    base_url: Option<String>,

    #[arg(long, global = true, help = "API key (optional for local servers)")]
    api_key: Option<String>,

    #[arg(long, global = true, help = "Force adapter: openai, anthropic, or ollama")]
    adapter: Option<String>,

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
    #[command(about = "Check self-hosted LLM setup: endpoint reachability, models, and recommendations")]
    Doctor,
    /// Start the web UI server
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value = "3000")]
        port: u16,
    },
    #[command(about = "Evaluate review quality against fixture expectations")]
    Eval {
        #[arg(long, default_value = "eval/fixtures")]
        fixtures: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(long, help = "Baseline eval JSON report to compare against")]
        baseline: Option<PathBuf>,

        #[arg(long, help = "Maximum allowed drop in micro-F1 vs baseline (0.0-1.0)")]
        max_micro_f1_drop: Option<f32>,

        #[arg(long, help = "Minimum required micro-F1 for current run (0.0-1.0)")]
        min_micro_f1: Option<f32>,

        #[arg(long, help = "Minimum required macro-F1 for current run (0.0-1.0)")]
        min_macro_f1: Option<f32>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Per-rule minimum F1 thresholds as rule_id=value (repeatable)"
        )]
        min_rule_f1: Vec<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Per-rule maximum allowed F1 drop vs baseline as rule_id=value (repeatable)"
        )]
        max_rule_f1_drop: Vec<String>,
    },
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
    if let Some(base_url) = cli.base_url {
        config.base_url = Some(base_url);
    }
    if let Some(api_key) = cli.api_key {
        config.api_key = Some(api_key);
    }
    if let Some(adapter) = cli.adapter {
        config.adapter = Some(adapter);
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
            commands::review_command(config, diff, patch, output, cli.output_format).await?;
        }
        Commands::Check { path } => {
            commands::check_command(path, config, cli.output_format).await?;
        }
        Commands::Git { command } => {
            commands::git_command(command, config, cli.output_format).await?;
        }
        Commands::Pr {
            number,
            repo,
            post_comments,
            summary,
        } => {
            commands::pr_command(
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
            commands::compare_command(old_file, new_file, config, cli.output_format).await?;
        }
        Commands::SmartReview { diff, output } => {
            commands::smart_review_command(config, diff, output).await?;
        }
        Commands::Changelog {
            from,
            to,
            release,
            output,
        } => {
            commands::changelog_command(from, to, release, output).await?;
        }
        Commands::LspCheck { path } => {
            commands::lsp_check_command(path, config).await?;
        }
        Commands::Feedback {
            accept,
            reject,
            feedback_path,
        } => {
            commands::feedback_command(config, accept, reject, feedback_path).await?;
        }
        Commands::Discuss {
            review,
            comment_id,
            comment_index,
            question,
            thread,
            interactive,
        } => {
            commands::discuss_command(
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
        Commands::Doctor => {
            commands::doctor_command(config).await?;
        }
        Commands::Serve { host, port } => {
            server::start_server(config, &host, port).await?;
        }
        Commands::Eval {
            fixtures,
            output,
            baseline,
            max_micro_f1_drop,
            min_micro_f1,
            min_macro_f1,
            min_rule_f1,
            max_rule_f1_drop,
        } => {
            let eval_options = EvalRunOptions {
                baseline_report: baseline,
                max_micro_f1_drop,
                min_micro_f1,
                min_macro_f1,
                min_rule_f1,
                max_rule_f1_drop,
            };
            commands::eval_command(config, fixtures, output, eval_options).await?;
        }
    }

    Ok(())
}
