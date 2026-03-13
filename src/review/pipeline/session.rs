#[path = "session/build.rs"]
mod build;
#[path = "session/semantic.rs"]
mod semantic;

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core;

use super::services::PipelineServices;
use super::types::ProgressCallback;
use build::build_review_session;

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
        build_review_session(diff_content, services, on_progress).await
    }
}
