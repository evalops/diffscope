use crate::core;

pub(super) fn score_context_chunks(
    diff: &core::UnifiedDiff,
    chunks: Vec<core::LLMContextChunk>,
) -> Vec<(i32, usize, core::LLMContextChunk)> {
    let changed_ranges = changed_ranges(diff);
    chunks
        .into_iter()
        .map(|chunk| {
            let score = score_context_chunk(diff, &changed_ranges, &chunk);
            (score, chunk.content.len(), chunk)
        })
        .collect()
}

fn changed_ranges(diff: &core::UnifiedDiff) -> Vec<(usize, usize)> {
    diff.hunks
        .iter()
        .filter(|hunk| hunk.new_lines > 0)
        .map(|hunk| {
            let start = hunk.new_start.max(1);
            let end = hunk.new_start.saturating_add(hunk.new_lines - 1).max(start);
            (start, end)
        })
        .collect()
}

fn score_context_chunk(
    diff: &core::UnifiedDiff,
    changed_ranges: &[(usize, usize)],
    chunk: &core::LLMContextChunk,
) -> i32 {
    let mut score = match chunk.context_type {
        core::ContextType::FileContent => 130,
        core::ContextType::Definition => 100,
        core::ContextType::Reference => 80,
        core::ContextType::Documentation => 60,
    };

    if chunk.file_path == diff.file_path {
        score += 90;
    }

    if let Some(range) = chunk.line_range {
        if changed_ranges
            .iter()
            .any(|candidate| ranges_overlap(*candidate, range))
        {
            score += 70;
        } else if chunk.file_path == diff.file_path {
            score += 20;
        }
    }

    if chunk.content.len() > 4000 {
        score -= 10;
    }

    if let Some(provenance) = chunk.provenance.as_ref() {
        score += provenance.ranking_bonus();
    }

    score
}

pub(super) fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}
