use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

use crate::core;

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    static SYMBOL_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\b([A-Z][a-zA-Z0-9_]*|[a-z][a-zA-Z0-9_]*)\s*\(").unwrap());
    static CLASS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\b(class|struct|interface|enum)\s+([A-Z][a-zA-Z0-9_]*)").unwrap()
    });

    for hunk in &diff.hunks {
        for line in &hunk.changes {
            if matches!(
                line.change_type,
                core::diff_parser::ChangeType::Added | core::diff_parser::ChangeType::Removed
            ) {
                for capture in SYMBOL_REGEX.captures_iter(&line.content) {
                    if let Some(symbol) = capture.get(1) {
                        let symbol_str = symbol.as_str().to_string();
                        if symbol_str.len() > 2 && seen.insert(symbol_str.clone()) {
                            symbols.push(symbol_str);
                        }
                    }
                }

                for capture in CLASS_REGEX.captures_iter(&line.content) {
                    if let Some(class_name) = capture.get(2) {
                        let class_str = class_name.as_str().to_string();
                        if seen.insert(class_str.clone()) {
                            symbols.push(class_str);
                        }
                    }
                }
            }
        }
    }

    symbols
}

#[cfg(test)]
mod tests {
    use super::*;
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
