use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::{Comment, ContextType, LLMContextChunk};

pub(super) fn supporting_context_for_comment(
    comment: &Comment,
    extra_context: &HashMap<PathBuf, Vec<LLMContextChunk>>,
) -> Vec<LLMContextChunk> {
    let mut chunks = extra_context
        .get(&comment.file_path)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|chunk| {
            !(chunk.file_path == comment.file_path
                && chunk.context_type == ContextType::FileContent)
        })
        .collect::<Vec<_>>();

    chunks.sort_by_key(|chunk| std::cmp::Reverse(score_supporting_context(chunk, comment)));
    chunks.truncate(3);
    chunks
}

fn score_supporting_context(chunk: &LLMContextChunk, comment: &Comment) -> i32 {
    let mut score = match chunk.context_type {
        ContextType::Definition => 90,
        ContextType::Reference => 70,
        ContextType::Documentation => 45,
        ContextType::FileContent => 20,
    };

    if chunk.file_path != comment.file_path {
        score += 15;
    }

    if let Some(range) = chunk.line_range {
        if comment.line_number >= range.0 && comment.line_number <= range.1 {
            score += 10;
        }
    }

    if let Some(provenance) = chunk.provenance.as_ref() {
        score += provenance.verification_bonus();
    }

    score
}
