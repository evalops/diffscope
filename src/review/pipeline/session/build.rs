use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

use crate::core;

use super::super::context::build_symbol_index;
use super::super::repo_support::{detect_instruction_files, gather_git_log};
use super::super::services::PipelineServices;
use super::super::types::ProgressCallback;
use super::semantic::{build_semantic_index, load_semantic_feedback_store};
use super::ReviewSession;

pub(super) async fn build_review_session(
    diff_content: &str,
    services: &PipelineServices,
    on_progress: Option<ProgressCallback>,
) -> Result<ReviewSession> {
    let diffs = parse_review_diffs(diff_content, services)?;
    let source_files = load_source_files(&diffs, services);

    let enhanced_ctx = build_enhanced_context(&diffs, &source_files, services);
    let enhanced_guidance = core::generate_enhanced_guidance(&enhanced_ctx, "rs");
    if !enhanced_guidance.is_empty() {
        info!(
            "Enhanced guidance generated ({} chars)",
            enhanced_guidance.len()
        );
    }

    Ok(ReviewSession {
        files_total: diffs.len(),
        symbol_index: build_symbol_index(&services.config, &services.repo_path),
        semantic_index: build_semantic_index(&diffs, services).await,
        semantic_feedback_store: load_semantic_feedback_store(services),
        auto_instructions: detect_auto_instructions(services),
        diffs,
        source_files,
        on_progress,
        enhanced_ctx,
        enhanced_guidance,
        verification_context: HashMap::new(),
    })
}

fn parse_review_diffs(
    diff_content: &str,
    services: &PipelineServices,
) -> Result<Vec<core::UnifiedDiff>> {
    let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
    info!("Parsed {} file diffs", diffs.len());

    if let Some(limit) = services.config.file_change_limit {
        if limit > 0 && diffs.len() > limit {
            anyhow::bail!(
                "Diff contains {} files, exceeding file_change_limit of {}. \
                 Increase the limit or split the review.",
                diffs.len(),
                limit
            );
        }
    }

    Ok(diffs)
}

fn load_source_files(
    diffs: &[core::UnifiedDiff],
    services: &PipelineServices,
) -> HashMap<PathBuf, String> {
    diffs
        .iter()
        .filter_map(|diff| {
            std::fs::read_to_string(services.repo_path.join(&diff.file_path))
                .ok()
                .map(|content| (diff.file_path.clone(), content))
        })
        .collect()
}

fn build_enhanced_context(
    diffs: &[core::UnifiedDiff],
    source_files: &HashMap<PathBuf, String>,
    services: &PipelineServices,
) -> crate::core::enhanced_review::EnhancedReviewContext {
    let git_log_output = gather_git_log(&services.repo_path);
    let convention_json = services
        .convention_store_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok());

    core::build_enhanced_context(
        diffs,
        source_files,
        git_log_output.as_deref(),
        None,
        convention_json.as_deref(),
        None,
    )
}

fn detect_auto_instructions(services: &PipelineServices) -> Option<String> {
    if !(services.config.auto_detect_instructions && services.config.review_instructions.is_none())
    {
        return None;
    }

    let detected = detect_instruction_files(&services.repo_path);
    if detected.is_empty() {
        None
    } else {
        Some(
            detected
                .iter()
                .map(|(name, content)| format!("# From {}\n{}", name, content))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    }
}
