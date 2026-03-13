use crate::core::diff_parser::UnifiedDiff;

/// Rough token estimation: ~4 chars per token (industry standard fallback).
pub(super) const CHARS_PER_TOKEN: usize = 4;

/// Estimate the token cost of a single diff.
pub fn estimate_diff_tokens(diff: &UnifiedDiff) -> usize {
    let chars: usize = diff.hunks.iter().map(hunk_char_count).sum();
    let file_overhead = diff.file_path.to_string_lossy().len() + 40;
    (chars + file_overhead) / CHARS_PER_TOKEN
}

pub(super) fn hunk_char_count(hunk: &crate::core::diff_parser::DiffHunk) -> usize {
    hunk.changes
        .iter()
        .map(|change| change.content.len() + 10)
        .sum::<usize>()
        + hunk.context.len()
        + 20
}
