use std::collections::HashSet;

use crate::core;

pub(super) fn dedupe_context_chunks(
    chunks: Vec<core::LLMContextChunk>,
) -> Vec<core::LLMContextChunk> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for chunk in chunks {
        if seen.insert(chunk_dedupe_key(&chunk)) {
            deduped.push(chunk);
        }
    }
    deduped
}

fn chunk_dedupe_key(chunk: &core::LLMContextChunk) -> String {
    format!(
        "{}|{:?}|{:?}|{:?}|{}",
        chunk.file_path.display(),
        chunk.context_type,
        chunk.line_range,
        chunk.provenance.as_ref().map(ToString::to_string),
        chunk.content
    )
}
