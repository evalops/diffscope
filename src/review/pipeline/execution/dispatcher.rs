#[path = "dispatcher/context.rs"]
mod context;
#[path = "dispatcher/job.rs"]
mod job;
#[path = "dispatcher/run.rs"]
mod run;

use anyhow::Result;
use std::path::PathBuf;

use crate::adapters;
use crate::core;

use super::super::types::AgentActivity;

pub(super) struct DispatchedJobResult {
    pub job_order: usize,
    pub diff_index: usize,
    pub active_rules: Vec<crate::core::ReviewRule>,
    pub path_config: Option<crate::config::PathConfig>,
    pub file_path: PathBuf,
    pub deterministic_comments: Vec<core::Comment>,
    pub pass_kind: Option<core::SpecializedPassKind>,
    pub mark_file_complete: bool,
    pub response: Result<adapters::llm::LLMResponse>,
    pub latency_ms: u64,
    pub agent_data: Option<AgentActivity>,
}

pub(super) use run::dispatch_jobs;
