use std::path::PathBuf;

use crate::core;
use crate::review::AgentActivity;

use super::{ProcessedJobResult, ResponseUsage};

pub(super) fn fallback_result(
    file_path: PathBuf,
    comments: Vec<core::Comment>,
    mark_file_complete: bool,
    latency_ms: u64,
    usage: ResponseUsage,
    agent_data: Option<AgentActivity>,
) -> ProcessedJobResult {
    let comment_count = comments.len();
    ProcessedJobResult {
        file_path,
        comments,
        latency_ms,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        comment_count,
        pass_tag: None,
        mark_file_complete,
        agent_data,
    }
}
