use std::collections::HashMap;
use std::path::PathBuf;

use crate::adapters;
use crate::config;
use crate::core;

use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::{AgentActivity, FileMetric};

pub(super) struct PreparedReviewJobs {
    pub jobs: Vec<FileReviewJob>,
    pub all_comments: Vec<core::Comment>,
    pub files_completed: usize,
    pub files_skipped: usize,
}

pub(super) struct FileReviewJob {
    pub job_order: usize,
    pub diff_index: usize,
    pub request: adapters::llm::LLMRequest,
    pub active_rules: Vec<core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub file_path: PathBuf,
    pub deterministic_comments: Vec<core::Comment>,
    pub pass_kind: Option<core::SpecializedPassKind>,
    pub mark_file_complete: bool,
}

pub(super) struct ReviewExecutionContext<'a> {
    pub services: &'a PipelineServices,
    pub session: &'a ReviewSession,
    pub initial_comments: Vec<core::Comment>,
    pub files_completed: usize,
    pub files_skipped: usize,
}

pub(super) struct ExecutionSummary {
    pub all_comments: Vec<core::Comment>,
    pub total_prompt_tokens: usize,
    pub total_completion_tokens: usize,
    pub total_tokens: usize,
    pub file_metrics: Vec<FileMetric>,
    pub comments_by_pass: HashMap<String, usize>,
    pub agent_activity: Option<AgentActivity>,
}
