mod adapters;
mod commands;
mod config;
mod core;
mod output;
mod parsing;
mod plugins;
mod review;
mod server;
mod vault;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
#[cfg(feature = "otel")]
use opentelemetry::trace::TracerProvider as _;
use std::path::PathBuf;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

use commands::{DagGraphSelection, EvalRunOptions, GitCommands};
use config::CliOverrides;
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "diffscope")]
#[command(about = "A composable code review engine with smart analysis and professional reporting", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true, default_value = "anthropic/claude-opus-4.5")]
    model: String,

    #[arg(
        long,
        global = true,
        help = "LLM API base URL (e.g. http://localhost:11434)"
    )]
    base_url: Option<String>,

    #[arg(long, global = true, help = "API key (optional for local servers)")]
    api_key: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Force adapter: openai, anthropic, openrouter, or ollama"
    )]
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

    #[arg(long, global = true, help = "HTTP timeout in seconds for LLM requests")]
    timeout: Option<u64>,

    #[arg(long, global = true, help = "Max retries on transient failures")]
    max_retries: Option<usize>,

    #[arg(long, global = true, help = "Skip review if diff exceeds N files")]
    file_change_limit: Option<usize>,

    #[arg(long, global = true, help = "Output language (e.g., en, ja, de)")]
    output_language: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Vault server address (e.g., https://vault:8200)"
    )]
    vault_addr: Option<String>,

    #[arg(long, global = true, help = "Vault secret path (e.g., diffscope)")]
    vault_path: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Key within Vault secret to use as API key (default: api_key)"
    )]
    vault_key: Option<String>,

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

    #[arg(
        long,
        global = true,
        help = "Enable agent loop for iterative tool-calling review"
    )]
    agent_review: bool,

    #[arg(
        long,
        global = true,
        help = "Max LLM round-trips in agent mode (default: 10)"
    )]
    agent_max_iterations: Option<usize>,

    #[arg(
        long,
        global = true,
        help = "Total token budget for agent loop (cost guard)"
    )]
    agent_max_total_tokens: Option<usize>,

    #[arg(
        long,
        global = true,
        help = "Enable or disable the verification pass (default: true)"
    )]
    verification_pass: Option<bool>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
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
    #[command(
        about = "Check self-hosted LLM setup: endpoint reachability, models, and recommendations"
    )]
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

        #[arg(
            long,
            help = "Maximum allowed micro-F1 drop for any shared suite vs baseline (0.0-1.0)"
        )]
        max_suite_f1_drop: Option<f32>,

        #[arg(
            long,
            help = "Maximum allowed micro-F1 drop for any shared category vs baseline (0.0-1.0)"
        )]
        max_category_f1_drop: Option<f32>,

        #[arg(
            long,
            help = "Maximum allowed micro-F1 drop for any shared language vs baseline (0.0-1.0)"
        )]
        max_language_f1_drop: Option<f32>,

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

        #[arg(
            long,
            value_delimiter = ',',
            help = "Additional model(s) to run as part of the eval matrix (repeatable)"
        )]
        matrix_model: Vec<String>,

        #[arg(
            long,
            default_value_t = 1,
            help = "Run each selected model this many times to measure flake"
        )]
        repeat: usize,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Only run benchmark-pack fixtures from the named suite(s)"
        )]
        suite: Vec<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Only run benchmark fixtures in the given category/categories"
        )]
        category: Vec<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Only run benchmark fixtures in the given language/languages"
        )]
        language: Vec<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Only run fixtures whose name contains one of these values"
        )]
        fixture_name: Vec<String>,

        #[arg(long, help = "Limit the number of fixtures executed after filtering")]
        max_fixtures: Option<usize>,

        #[arg(long, help = "Optional label attached to the eval report")]
        label: Option<String>,

        #[arg(long, help = "Append benchmark summary to this QualityTrend JSON file")]
        trend_file: Option<PathBuf>,

        #[arg(
            long,
            help = "Write failed-fixture artifacts and per-run reports under this directory"
        )]
        artifact_dir: Option<PathBuf>,

        #[arg(
            long,
            default_value_t = false,
            help = "Allow eval runs with non-frontier review/judge models"
        )]
        allow_subfrontier_models: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Run a tool-using reproduction validator over emitted comments"
        )]
        repro_validate: bool,

        #[arg(
            long,
            default_value_t = 3,
            help = "Maximum number of comments per fixture to send through reproduction validation"
        )]
        repro_max_comments: usize,
    },
    #[command(about = "Evaluate accepted/rejected human feedback from stored review data")]
    FeedbackEval {
        #[arg(
            help = "Path to reviews.json, a labeled comments JSON file, or semantic feedback store JSON"
        )]
        input: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(
            long,
            default_value_t = 0.75,
            help = "Confidence threshold used for acceptance calibration (0.0-1.0)"
        )]
        confidence_threshold: f32,

        #[arg(
            long,
            help = "Optional eval JSON report to correlate with feedback outcomes"
        )]
        eval_report: Option<PathBuf>,
    },
    #[command(about = "Print pipeline DAG contracts for orchestration and planning")]
    Dag {
        #[command(subcommand)]
        command: DagCommands,
    },
}

#[derive(Subcommand)]
enum DagCommands {
    #[command(about = "Describe the top-level review pipeline DAG")]
    Review,
    #[command(about = "Describe the granular review postprocess DAG")]
    Postprocess {
        #[arg(
            long,
            default_value_t = false,
            help = "Describe the graph as if convention-store persistence is enabled"
        )]
        convention_store_path: bool,
    },
    #[command(about = "Describe the eval fixture execution DAG")]
    Eval {
        #[arg(
            long,
            default_value_t = false,
            help = "Describe the graph with reproduction validation enabled"
        )]
        repro_validate: bool,
    },
    #[command(about = "Describe the full DAG catalog with nested graph references")]
    Catalog {
        #[arg(
            long,
            default_value_t = false,
            help = "Describe the postprocess graph as if convention-store persistence is enabled"
        )]
        convention_store_path: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Describe the eval graph with reproduction validation enabled"
        )]
        repro_validate: bool,
    },
    #[command(about = "Plan which DAG nodes are ready given a completed set")]
    Ready {
        #[arg(long, value_enum, help = "Graph to plan")]
        graph: DagGraphKind,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated completed node names"
        )]
        completed: Vec<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Plan the postprocess graph as if convention-store persistence is enabled"
        )]
        convention_store_path: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Plan the eval graph with reproduction validation enabled"
        )]
        repro_validate: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DagGraphKind {
    Review,
    Postprocess,
    Eval,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    #[cfg(feature = "otel")]
    let _otel_guard: Option<opentelemetry_sdk::trace::TracerProvider> = {
        let otel_enabled = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok();
        if otel_enabled {
            match opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .build()
            {
                Ok(exporter) => {
                    let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
                        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
                        .with_resource(opentelemetry_sdk::Resource::new(vec![
                            opentelemetry::KeyValue::new("service.name", "diffscope"),
                        ]))
                        .build();

                    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

                    let otel_layer = tracing_opentelemetry::layer()
                        .with_tracer(tracer_provider.tracer("diffscope"));

                    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
                        .with_env_filter(filter)
                        .finish()
                        .with(otel_layer);

                    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
                        eprintln!("Warning: failed to set OTEL tracing subscriber: {}", e);
                        // Already initialized by another thread or test — not fatal
                    }

                    Some(tracer_provider)
                }
                Err(e) => {
                    eprintln!(
                        "Warning: OTEL_EXPORTER_OTLP_ENDPOINT set but exporter failed to initialize: {}. Continuing without OpenTelemetry.",
                        e
                    );
                    tracing_subscriber::fmt().with_env_filter(filter).init();
                    None
                }
            }
        } else {
            tracing_subscriber::fmt().with_env_filter(filter).init();
            None
        }
    };

    #[cfg(not(feature = "otel"))]
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Load configuration from file and merge with CLI options
    let mut config = config::Config::load().unwrap_or_default();
    config.merge_with_cli(Some(cli.model.clone()), cli.prompt.clone());

    // Override config with CLI options
    config.apply_cli_overrides(CliOverrides {
        temperature: cli.temperature,
        max_tokens: cli.max_tokens,
        strictness: cli.strictness,
        comment_types: cli.comment_types,
        openai_responses: cli.openai_responses,
        base_url: cli.base_url,
        api_key: cli.api_key,
        adapter: cli.adapter,
        lsp_command: cli.lsp_command,
        timeout: cli.timeout,
        max_retries: cli.max_retries,
        file_change_limit: cli.file_change_limit,
        output_language: cli.output_language,
        vault_addr: cli.vault_addr,
        vault_path: cli.vault_path,
        vault_key: cli.vault_key,
        agent_review: cli.agent_review,
        agent_max_iterations: cli.agent_max_iterations,
        agent_max_total_tokens: cli.agent_max_total_tokens,
        verification_pass: cli.verification_pass,
    });
    config.normalize();

    // Resolve API key from Vault if configured and api_key is not already set
    if let Err(e) = config.resolve_vault_api_key().await {
        eprintln!("Warning: Failed to fetch API key from Vault: {e:#}");
    }

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
            max_suite_f1_drop,
            max_category_f1_drop,
            max_language_f1_drop,
            min_micro_f1,
            min_macro_f1,
            min_rule_f1,
            max_rule_f1_drop,
            matrix_model,
            repeat,
            suite,
            category,
            language,
            fixture_name,
            max_fixtures,
            label,
            trend_file,
            artifact_dir,
            allow_subfrontier_models,
            repro_validate,
            repro_max_comments,
        } => {
            let eval_options = EvalRunOptions {
                baseline_report: baseline,
                max_micro_f1_drop,
                max_suite_f1_drop,
                max_category_f1_drop,
                max_language_f1_drop,
                min_micro_f1,
                min_macro_f1,
                min_rule_f1,
                max_rule_f1_drop,
                matrix_models: matrix_model,
                repeat,
                suite_filters: suite,
                category_filters: category,
                language_filters: language,
                fixture_name_filters: fixture_name,
                max_fixtures,
                label,
                trend_file,
                artifact_dir,
                allow_subfrontier_models,
                repro_validate,
                repro_max_comments,
            };
            commands::eval_command(config, fixtures, output, eval_options).await?;
        }
        Commands::FeedbackEval {
            input,
            output,
            confidence_threshold,
            eval_report,
        } => {
            commands::feedback_eval_command(input, output, confidence_threshold, eval_report)
                .await?;
        }
        Commands::Dag { command } => match command {
            DagCommands::Review => {
                let graph = commands::describe_dag_graph(&config, DagGraphSelection::Review);
                println!("{}", serde_json::to_string_pretty(&graph)?);
            }
            DagCommands::Postprocess {
                convention_store_path,
            } => {
                let graph = commands::describe_dag_graph(
                    &config,
                    DagGraphSelection::Postprocess {
                        convention_store_path,
                    },
                );
                println!("{}", serde_json::to_string_pretty(&graph)?);
            }
            DagCommands::Eval { repro_validate } => {
                let graph = commands::describe_dag_graph(
                    &config,
                    DagGraphSelection::Eval { repro_validate },
                );
                println!("{}", serde_json::to_string_pretty(&graph)?);
            }
            DagCommands::Catalog {
                convention_store_path,
                repro_validate,
            } => {
                let catalog =
                    commands::build_dag_catalog(&config, repro_validate, convention_store_path);
                println!("{}", serde_json::to_string_pretty(&catalog)?);
            }
            DagCommands::Ready {
                graph,
                completed,
                convention_store_path,
                repro_validate,
            } => {
                let selection = match graph {
                    DagGraphKind::Review => DagGraphSelection::Review,
                    DagGraphKind::Postprocess => DagGraphSelection::Postprocess {
                        convention_store_path,
                    },
                    DagGraphKind::Eval => DagGraphSelection::Eval { repro_validate },
                };
                let plan = commands::plan_dag_graph(&config, selection, &completed)?;
                println!("{}", serde_json::to_string_pretty(&plan)?);
            }
        },
    }

    #[cfg(feature = "otel")]
    if let Some(ref provider) = _otel_guard {
        let _ = provider.shutdown();
    }

    Ok(())
}
