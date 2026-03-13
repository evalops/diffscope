#[path = "symbols/collect.rs"]
mod collect;
#[path = "symbols/patterns.rs"]
mod patterns;

pub use collect::extract_symbols_from_diff;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core;
    use std::path::PathBuf;

    #[test]
    fn extract_symbols_from_diff_finds_functions() {
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
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "let result = process_data(input);".to_string(),
                    change_type: core::diff_parser::ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.contains(&"process_data".to_string()));
    }

    #[test]
    fn extract_symbols_from_diff_finds_classes() {
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
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "struct MyHandler {".to_string(),
                    change_type: core::diff_parser::ChangeType::Added,
                    old_line_no: None,
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.contains(&"MyHandler".to_string()));
    }

    #[test]
    fn extract_symbols_ignores_context_lines() {
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
                new_start: 1,
                new_lines: 1,
                changes: vec![core::diff_parser::DiffLine {
                    content: "let x = unchanged_func(y);".to_string(),
                    change_type: core::diff_parser::ChangeType::Context,
                    old_line_no: Some(1),
                    new_line_no: Some(1),
                }],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert!(symbols.is_empty());
    }

    #[test]
    fn extract_symbols_preserves_insertion_order() {
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
                old_lines: 0,
                new_start: 1,
                new_lines: 3,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "alpha(1);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "beta(2);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                    core::diff_parser::DiffLine {
                        content: "gamma(3); alpha(4);".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(3),
                    },
                ],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert_eq!(
            symbols,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn extract_symbols_deduplicates() {
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
                old_lines: 0,
                new_start: 1,
                new_lines: 2,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "handle_user();".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "handle_user();".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                ],
            }],
        };
        let symbols = extract_symbols_from_diff(&diff);
        assert_eq!(symbols, vec!["handle_user".to_string()]);
    }
}
