use anyhow::Result;
use std::path::Path;

use crate::config;

use super::repo_support::chunk_diff_for_context;
use super::services::should_optimize_for_local;
use super::types::{ProgressCallback, ReviewResult};

pub(super) async fn maybe_review_chunked_diff_content(
    diff_content: &str,
    config: config::Config,
    repo_path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<Option<ReviewResult>> {
    if !should_optimize_for_local(&config) {
        return Ok(None);
    }

    let context_budget = config.context_window.unwrap_or(8192);
    let max_diff_chars = (context_budget * 2 / 5).max(1000);
    let chunks = chunk_diff_for_context(diff_content, max_diff_chars);
    if chunks.len() <= 1 {
        return Ok(None);
    }

    eprintln!(
        "Diff split into {} chunks for local model context window",
        chunks.len()
    );

    let mut merged = ReviewResult::default();
    for (index, chunk) in chunks.iter().enumerate() {
        eprintln!("Processing chunk {}/{}...", index + 1, chunks.len());
        match super::orchestrate::review_diff_content_raw_inner(
            chunk,
            config.clone(),
            repo_path,
            on_progress.clone(),
        )
        .await
        {
            Ok(chunk_result) => merge_chunk_result(&mut merged, chunk_result),
            Err(error) => {
                eprintln!("Warning: chunk {} failed: {}", index + 1, error);
            }
        }
    }

    Ok(Some(merged))
}

fn merge_chunk_result(merged: &mut ReviewResult, chunk_result: ReviewResult) {
    merged.comments.extend(chunk_result.comments);
    merged.total_prompt_tokens += chunk_result.total_prompt_tokens;
    merged.total_completion_tokens += chunk_result.total_completion_tokens;
    merged.total_tokens += chunk_result.total_tokens;
    merged.file_metrics.extend(chunk_result.file_metrics);
    merged.convention_suppressed_count += chunk_result.convention_suppressed_count;
    for (pass, count) in chunk_result.comments_by_pass {
        *merged.comments_by_pass.entry(pass).or_insert(0) += count;
    }
    merged.hotspots.extend(chunk_result.hotspots);
    merged.warnings.extend(chunk_result.warnings);
}
