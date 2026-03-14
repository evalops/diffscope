use crate::core::diff_parser::{ChangeType, DiffLine};
use crate::core::UnifiedDiff;

pub(super) fn diff_snippet_for_comment(diff: &UnifiedDiff, line_number: usize) -> String {
    for hunk in &diff.hunks {
        if hunk
            .changes
            .iter()
            .any(|change| line_matches_change(change, line_number))
        {
            return hunk
                .changes
                .iter()
                .map(render_change)
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}

pub(super) fn line_is_removed_only_in_diff(diff: &UnifiedDiff, line_number: usize) -> bool {
    let mut has_old = false;
    let mut has_new = false;

    for hunk in &diff.hunks {
        for change in &hunk.changes {
            if change.old_line_no == Some(line_number) {
                has_old = true;
            }
            if change.new_line_no == Some(line_number) {
                has_new = true;
            }
        }
    }

    has_old && !has_new
}

pub(super) fn source_context_for_line(content: &str, line_number: usize, radius: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let target_line = line_number.clamp(1, lines.len());
    let start = target_line.saturating_sub(radius + 1);
    let end = (target_line + radius).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{:>4}: {}", start + offset + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_matches_change(change: &DiffLine, line_number: usize) -> bool {
    change.new_line_no == Some(line_number) || change.old_line_no == Some(line_number)
}

fn render_change(change: &DiffLine) -> String {
    let (prefix, line_number) = match change.change_type {
        ChangeType::Added => ("+", change.new_line_no),
        ChangeType::Removed => ("-", change.old_line_no),
        ChangeType::Context => (" ", change.new_line_no.or(change.old_line_no)),
    };
    let line_number = line_number.unwrap_or(0);
    format!("{}{line_number:>4}: {}", prefix, change.content)
}
