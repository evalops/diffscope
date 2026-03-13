use std::collections::HashSet;

use crate::core;

use super::patterns::{class_regex, symbol_regex};

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    for hunk in &diff.hunks {
        for line in &hunk.changes {
            if !matches!(
                line.change_type,
                core::diff_parser::ChangeType::Added | core::diff_parser::ChangeType::Removed
            ) {
                continue;
            }

            add_captured_symbols(&line.content, symbol_regex(), 1, 3, &mut seen, &mut symbols);
            add_captured_symbols(&line.content, class_regex(), 2, 0, &mut seen, &mut symbols);
        }
    }

    symbols
}

fn add_captured_symbols(
    content: &str,
    regex: &regex::Regex,
    capture_index: usize,
    min_len: usize,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<String>,
) {
    for capture in regex.captures_iter(content) {
        let Some(symbol) = capture.get(capture_index) else {
            continue;
        };
        push_symbol(symbol.as_str(), min_len, seen, symbols);
    }
}

fn push_symbol(
    symbol: &str,
    min_len: usize,
    seen: &mut HashSet<String>,
    symbols: &mut Vec<String>,
) {
    if symbol.len() <= min_len {
        return;
    }

    let symbol = symbol.to_string();
    if seen.insert(symbol.clone()) {
        symbols.push(symbol);
    }
}
