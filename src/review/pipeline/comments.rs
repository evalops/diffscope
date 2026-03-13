use anyhow::Result;
use tracing::info;

use crate::core;
use crate::plugins;

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

pub(super) fn synthesize_analyzer_comments(
    findings: Vec<plugins::AnalyzerFinding>,
) -> Result<Vec<core::Comment>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }

    let raw_comments = findings
        .into_iter()
        .map(|finding| finding.into_raw_comment())
        .collect::<Vec<_>>();
    core::CommentSynthesizer::synthesize(raw_comments)
}

pub(super) fn is_analyzer_comment(comment: &core::Comment) -> bool {
    comment.tags.iter().any(|tag| tag.starts_with("source:"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_line_in_diff_basic() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 5,
                new_lines: 2,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "old".to_string(),
                        change_type: core::diff_parser::ChangeType::Removed,
                        old_line_no: Some(1),
                        new_line_no: None,
                    },
                    core::diff_parser::DiffLine {
                        content: "new".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(5),
                    },
                ],
            }],
        };

        assert!(is_line_in_diff(&diff, 5));
        assert!(!is_line_in_diff(&diff, 6));
        assert!(!is_line_in_diff(&diff, 0));
    }
}
