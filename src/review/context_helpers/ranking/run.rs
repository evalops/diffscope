use crate::core;

use super::dedupe::dedupe_context_chunks;
use super::scoring::score_context_chunks;
use super::selection::select_ranked_chunks;

pub fn rank_and_trim_context_chunks(
    diff: &core::UnifiedDiff,
    chunks: Vec<core::LLMContextChunk>,
    max_chunks: usize,
    max_chars: usize,
) -> Vec<core::LLMContextChunk> {
    if chunks.is_empty() {
        return chunks;
    }

    let deduped = dedupe_context_chunks(chunks);
    let scored = score_context_chunks(diff, deduped);
    let kept = select_ranked_chunks(scored, max_chunks, max_chars);

    if kept.is_empty() {
        Vec::new()
    } else {
        kept
    }
}
