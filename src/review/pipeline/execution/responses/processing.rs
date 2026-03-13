#[path = "processing/comments.rs"]
mod comments;
#[path = "processing/fallback.rs"]
mod fallback;
#[path = "processing/merge.rs"]
mod merge;
#[path = "processing/run.rs"]
mod run;
#[path = "processing/usage.rs"]
mod usage;

use std::path::PathBuf;

use crate::core;
use crate::review::AgentActivity;

pub(in super::super) struct ProcessedJobResult {
    pub file_path: PathBuf,
    pub comments: Vec<core::Comment>,
    pub latency_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub comment_count: usize,
    pub pass_tag: Option<String>,
    pub mark_file_complete: bool,
    pub agent_data: Option<AgentActivity>,
}

#[derive(Default)]
struct ResponseUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

pub(in super::super) use run::process_job_result;
