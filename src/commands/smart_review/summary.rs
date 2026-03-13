use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::core::pr_summary::PRSummary;

pub(super) async fn build_pr_summary(
    config: &config::Config,
    repo_root: &Path,
    diffs: &[core::UnifiedDiff],
    primary_adapter: &dyn adapters::llm::LLMAdapter,
) -> Result<Option<PRSummary>> {
    let model_config = config.to_model_config();
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
    let summary_adapter = separate_fast_adapter.as_deref().unwrap_or(primary_adapter);

    let mut pr_summary = if config.smart_review_summary {
        match core::GitIntegration::new(repo_root) {
            Ok(git) => {
                let options = core::SummaryOptions {
                    include_diagram: false,
                };
                match core::PRSummaryGenerator::generate_summary_with_options(
                    diffs,
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
        match core::PRSummaryGenerator::generate_change_diagram(diffs, summary_adapter).await {
            Ok(Some(diagram)) => {
                if let Some(summary) = &mut pr_summary {
                    summary.visual_diff = Some(diagram);
                } else {
                    pr_summary = Some(core::PRSummaryGenerator::build_diagram_only_summary(
                        diffs, diagram,
                    ));
                }
            }
            Ok(None) => {}
            Err(err) => warn!("Diagram generation failed: {}", err),
        }
    }

    Ok(pr_summary)
}
