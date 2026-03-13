use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::config;
use crate::core;

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();
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

pub fn build_symbol_index(config: &config::Config, repo_root: &Path) -> Option<core::SymbolIndex> {
    if !config.symbol_index {
        return None;
    }

    let provider = config.symbol_index_provider.as_str();
    let result = if provider == "lsp" {
        let detected_command = if config.symbol_index_lsp_command.is_none() {
            core::SymbolIndex::detect_lsp_command(
                repo_root,
                config.symbol_index_max_files,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            )
        } else {
            None
        };

        let command = config
            .symbol_index_lsp_command
            .as_deref()
            .map(str::to_string)
            .or(detected_command);

        if let Some(command) = command {
            if config.symbol_index_lsp_command.is_none() {
                info!("Auto-detected LSP command: {}", command);
            }

            match core::SymbolIndex::build_with_lsp(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                &command,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            ) {
                Ok(index) => Ok(index),
                Err(err) => {
                    warn!("LSP indexer failed (falling back to regex): {}", err);
                    core::SymbolIndex::build(
                        repo_root,
                        config.symbol_index_max_files,
                        config.symbol_index_max_bytes,
                        config.symbol_index_max_locations,
                        |path| config.should_exclude(path),
                    )
                }
            }
        } else {
            warn!("No LSP command configured or detected; falling back to regex indexer.");
            core::SymbolIndex::build(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                |path| config.should_exclude(path),
            )
        }
    } else {
        core::SymbolIndex::build(
            repo_root,
            config.symbol_index_max_files,
            config.symbol_index_max_bytes,
            config.symbol_index_max_locations,
            |path| config.should_exclude(path),
        )
    };

    match result {
        Ok(index) => {
            info!(
                "Indexed {} symbols across {} files",
                index.symbols_indexed(),
                index.files_indexed()
            );
            Some(index)
        }
        Err(err) => {
            warn!("Symbol index build failed: {}", err);
            None
        }
    }
}

pub(super) fn gather_related_file_context(
    index: &core::SymbolIndex,
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    let mut chunks: Vec<core::LLMContextChunk> = Vec::new();

    let callers = index.reverse_deps(file_path);
    for caller_path in callers.iter().take(3) {
        if let Some(summary) = index.file_summary(caller_path) {
            let truncated: String = if summary.len() > 2000 {
                let mut end = 2000;
                while end > 0 && !summary.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &summary[..end])
            } else {
                summary.to_string()
            };
            chunks.push(
                core::LLMContextChunk::reference(
                    caller_path.clone(),
                    format!("[Caller/dependent file]\n{}", truncated),
                )
                .with_provenance(core::ContextProvenance::ReverseDependencySummary),
            );
        }
    }

    let test_files = find_test_files(file_path, repo_path);
    for test_path in test_files.iter().take(2) {
        let relative: &Path = test_path.strip_prefix(repo_path).unwrap_or(test_path);
        if let Ok(content) = std::fs::read_to_string(test_path) {
            let snippet: String = content.lines().take(60).collect::<Vec<_>>().join("\n");
            if !snippet.is_empty() {
                chunks.push(
                    core::LLMContextChunk::reference(
                        relative.to_path_buf(),
                        format!("[Test file]\n{}", snippet),
                    )
                    .with_provenance(core::ContextProvenance::RelatedTestFile),
                );
            }
        }
    }

    chunks
}

fn find_test_files(file_path: &Path, repo_path: &Path) -> Vec<PathBuf> {
    let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = file_path.parent().unwrap_or(Path::new(""));

    let candidates: Vec<PathBuf> = vec![
        repo_path
            .join(parent)
            .join(format!("test_{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}_test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.spec.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("tests")
            .join(format!("{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("__tests__")
            .join(format!("{}.{}", stem, ext)),
    ];

    candidates
        .into_iter()
        .filter(|path: &PathBuf| path.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_utf8_safe_truncation() {
        let prefix = "a".repeat(1999);
        let value = format!("{}€rest", prefix);
        assert!(value.len() > 2000);

        let mut end = 2000;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &value[..end];

        assert_eq!(end, 1999);
        assert!(truncated.starts_with("aaa"));
        assert!(!truncated.contains('€'));
    }
}
