use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::core;

use super::context::build_symbol_index;
use super::repo_support::{detect_instruction_files, gather_git_log};
use super::services::PipelineServices;
use super::types::ProgressCallback;

pub(super) struct ReviewSession {
    pub diffs: Vec<core::UnifiedDiff>,
    pub source_files: HashMap<PathBuf, String>,
    pub files_total: usize,
    pub on_progress: Option<ProgressCallback>,
    pub enhanced_ctx: crate::core::enhanced_review::EnhancedReviewContext,
    pub enhanced_guidance: String,
    pub auto_instructions: Option<String>,
    pub symbol_index: Option<core::SymbolIndex>,
    pub semantic_index: Option<core::semantic::SemanticIndex>,
    pub semantic_feedback_store: Option<core::SemanticFeedbackStore>,
    pub verification_context: HashMap<PathBuf, Vec<core::LLMContextChunk>>,
}

impl ReviewSession {
    pub(super) async fn new(
        diff_content: &str,
        services: &PipelineServices,
        on_progress: Option<ProgressCallback>,
    ) -> Result<Self> {
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

        let source_files: HashMap<PathBuf, String> = diffs
            .iter()
            .filter_map(|diff| {
                std::fs::read_to_string(services.repo_path.join(&diff.file_path))
                    .ok()
                    .map(|content| (diff.file_path.clone(), content))
            })
            .collect();

        let git_log_output = gather_git_log(&services.repo_path);
        let convention_json = services
            .convention_store_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());

        let enhanced_ctx = core::build_enhanced_context(
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

        let auto_instructions = if services.config.auto_detect_instructions
            && services.config.review_instructions.is_none()
        {
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
        } else {
            None
        };

        let symbol_index = build_symbol_index(&services.config, &services.repo_path);

        let semantic_index = if services.config.semantic_rag {
            let index_path = core::default_index_path(&services.repo_path);
            let changed_files = diffs
                .iter()
                .map(|diff| diff.file_path.clone())
                .collect::<Vec<_>>();
            match core::refresh_semantic_index(
                &services.repo_path,
                &index_path,
                services.embedding_adapter.as_deref(),
                &changed_files,
                |path| services.config.should_exclude(path),
                services.config.semantic_rag_max_files,
            )
            .await
            {
                Ok(index) => Some(index),
                Err(error) => {
                    warn!("Semantic index refresh failed: {}", error);
                    None
                }
            }
        } else {
            None
        };

        let semantic_feedback_store = if services.config.semantic_feedback {
            let path = core::default_semantic_feedback_path(&services.config.feedback_path);
            let mut store = core::load_semantic_feedback_store(&path);
            core::align_semantic_feedback_store(&mut store, services.embedding_adapter.as_deref());
            Some(store)
        } else {
            None
        };

        Ok(Self {
            files_total: diffs.len(),
            diffs,
            source_files,
            on_progress,
            enhanced_ctx,
            enhanced_guidance,
            auto_instructions,
            symbol_index,
            semantic_index,
            semantic_feedback_store,
            verification_context: HashMap::new(),
        })
    }
}
