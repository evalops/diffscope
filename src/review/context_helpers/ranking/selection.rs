use std::cmp::Reverse;

use crate::core;

pub(super) fn select_ranked_chunks(
    mut scored: Vec<(i32, usize, core::LLMContextChunk)>,
    max_chunks: usize,
    max_chars: usize,
) -> Vec<core::LLMContextChunk> {
    scored.sort_by_key(|(score, len, _)| (Reverse(*score), *len));

    let mut kept = Vec::new();
    let mut used_chars = 0usize;
    let max_chunks = normalize_limit(max_chunks);
    let max_chars = normalize_limit(max_chars);

    for (_, _, chunk) in scored {
        if kept.len() >= max_chunks {
            break;
        }

        let chunk_len = chunk.content.len();
        if used_chars.saturating_add(chunk_len) > max_chars {
            continue;
        }

        used_chars = used_chars.saturating_add(chunk_len);
        kept.push(chunk);
    }

    kept
}

fn normalize_limit(limit: usize) -> usize {
    if limit == 0 {
        usize::MAX
    } else {
        limit
    }
}
