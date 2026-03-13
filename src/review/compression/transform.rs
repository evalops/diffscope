use crate::core::diff_parser::{ChangeType, DiffHunk, UnifiedDiff};

use super::metrics::hunk_char_count;

/// Check if a hunk is deletion-only (all changes are removals or context).
pub fn is_deletion_only_hunk(hunk: &DiffHunk) -> bool {
    hunk.changes.iter().all(|change| {
        change.change_type == ChangeType::Removed || change.change_type == ChangeType::Context
    })
}

/// Remove deletion-only hunks from a diff. Returns a new diff (or None if all hunks removed).
pub fn compress_diff(diff: &UnifiedDiff) -> Option<UnifiedDiff> {
    let kept_hunks: Vec<DiffHunk> = diff
        .hunks
        .iter()
        .filter(|hunk| !is_deletion_only_hunk(hunk))
        .cloned()
        .collect();

    rebuild_diff(diff, kept_hunks)
}

/// Clip a diff to fit within a character budget by keeping only leading hunks.
pub fn clip_diff(diff: &UnifiedDiff, max_chars: usize) -> Option<UnifiedDiff> {
    let mut kept_hunks = Vec::new();
    let mut chars_used = 0usize;

    for hunk in &diff.hunks {
        let hunk_chars = hunk_char_count(hunk);
        if chars_used + hunk_chars > max_chars && !kept_hunks.is_empty() {
            break;
        }
        kept_hunks.push(hunk.clone());
        chars_used += hunk_chars;
    }

    rebuild_diff(diff, kept_hunks)
}

fn rebuild_diff(diff: &UnifiedDiff, hunks: Vec<DiffHunk>) -> Option<UnifiedDiff> {
    if hunks.is_empty() {
        return None;
    }

    Some(UnifiedDiff {
        file_path: diff.file_path.clone(),
        old_content: diff.old_content.clone(),
        new_content: diff.new_content.clone(),
        hunks,
        is_binary: diff.is_binary,
        is_deleted: diff.is_deleted,
        is_new: diff.is_new,
    })
}
