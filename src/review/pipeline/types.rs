use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core;

#[derive(Debug, Clone)]
pub struct AgentActivity {
    pub total_iterations: usize,
    pub tool_calls: Vec<core::agent_loop::AgentToolCallLog>,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewResult {
    pub comments: Vec<core::Comment>,
    pub total_prompt_tokens: usize,
    pub total_completion_tokens: usize,
    pub total_tokens: usize,
    pub file_metrics: Vec<FileMetric>,
    pub convention_suppressed_count: usize,
    pub comments_by_pass: HashMap<String, usize>,
    pub hotspots: Vec<core::multi_pass::HotspotResult>,
    pub agent_activity: Option<AgentActivity>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetric {
    pub file_path: PathBuf,
    pub latency_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub comment_count: usize,
}

pub struct ProgressUpdate {
    pub current_file: String,
    pub files_total: usize,
    pub files_completed: usize,
    pub files_skipped: usize,
    pub comments_so_far: Vec<core::Comment>,
}

pub type ProgressCallback = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;
