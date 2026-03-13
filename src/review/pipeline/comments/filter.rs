use tracing::info;

use crate::core;

pub fn filter_comments_for_diff(
    diff: &core::UnifiedDiff,
    comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    let mut filtered = Vec::new();
    let total = comments.len();
    for comment in comments {
        if is_line_in_diff(diff, comment.line_number) {
            filtered.push(comment);
        }
    }

    if filtered.len() != total {
        let dropped = total.saturating_sub(filtered.len());
        info!(
            "Dropped {} comment(s) for {} due to unmatched line numbers",
            dropped,
            diff.file_path.display()
        );
    }

    filtered
}

pub fn is_line_in_diff(diff: &core::UnifiedDiff, line_number: usize) -> bool {
    if line_number == 0 {
        return false;
    }
    diff.hunks.iter().any(|hunk| {
        hunk.changes
            .iter()
            .any(|line| line.new_line_no == Some(line_number))
    })
}
