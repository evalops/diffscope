use anyhow::Result;

use crate::core;

use super::super::services::PipelineServices;

pub(super) async fn initial_context_chunks(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    pre_analysis_context: Vec<core::LLMContextChunk>,
) -> Result<Vec<core::LLMContextChunk>> {
    let mut context_chunks = services
        .context_fetcher
        .fetch_context_for_file(&diff.file_path, &changed_line_ranges(diff))
        .await?;
    context_chunks.extend(pre_analysis_context);
    Ok(context_chunks)
}

fn changed_line_ranges(diff: &core::UnifiedDiff) -> Vec<(usize, usize)> {
    diff.hunks
        .iter()
        .map(|hunk| {
            (
                hunk.new_start,
                hunk.new_start + hunk.new_lines.saturating_sub(1),
            )
        })
        .collect()
}
