use crate::core::diff_parser::{ChangeType, DiffLine, UnifiedDiff};

pub(super) fn collect_non_context_changes(diff: &UnifiedDiff) -> Vec<&DiffLine> {
    diff.hunks
        .iter()
        .flat_map(|hunk| hunk.changes.iter())
        .filter(|change| !matches!(change.change_type, ChangeType::Context))
        .collect()
}

pub(super) fn is_deletion_only_change(changes: &[&DiffLine]) -> bool {
    changes
        .iter()
        .all(|change| matches!(change.change_type, ChangeType::Removed))
}

pub(super) fn is_whitespace_only_change(changes: &[&DiffLine]) -> bool {
    let removed: Vec<&str> = changes
        .iter()
        .filter(|change| matches!(change.change_type, ChangeType::Removed))
        .map(|change| change.content.as_str())
        .collect();

    let added: Vec<&str> = changes
        .iter()
        .filter(|change| matches!(change.change_type, ChangeType::Added))
        .map(|change| change.content.as_str())
        .collect();

    if removed.len() != added.len() {
        return false;
    }

    removed
        .iter()
        .zip(added.iter())
        .all(|(removed, added)| strip_whitespace(removed) == strip_whitespace(added))
}

fn strip_whitespace(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}
