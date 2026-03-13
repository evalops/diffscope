use anyhow::Result;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::{build_change_walkthrough, format_smart_review_output};
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

    let (repo_root, diff_content) = super::review::load_review_input(diff_path).await?;
    if diff_content.trim().is_empty() {
        return Ok(());
    }

    let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
    info!("Parsed {} file diffs", diffs.len());
    let walkthrough = build_change_walkthrough(&diffs);

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
    let review_result =
        review::review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;
    let processed_comments = review_result.comments;

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
