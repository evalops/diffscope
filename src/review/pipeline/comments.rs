#[path = "comments/analyzer.rs"]
mod analyzer;
#[path = "comments/filter.rs"]
mod filter;

pub(super) use analyzer::{is_analyzer_comment, synthesize_analyzer_comments};
pub use filter::{filter_comments_for_diff, is_line_in_diff};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core;
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
