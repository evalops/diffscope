use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine, UnifiedDiff};

/// A chunk of diff changes scoped to a single function/method.
#[derive(Debug, Clone)]
pub struct FunctionChunk {
    pub function_name: String,
    pub file_path: std::path::PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub language: String,
    pub changes: Vec<DiffLine>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub context_lines: usize,
}

impl FunctionChunk {
    pub fn total_changes(&self) -> usize {
        self.added_lines + self.removed_lines
    }

    pub fn change_density(&self) -> f32 {
        let span = (self.end_line.saturating_sub(self.start_line) + 1) as f32;
        if span == 0.0 {
            return 0.0;
        }
        self.total_changes() as f32 / span
    }
}

/// A detected function boundary in source code.
#[derive(Debug, Clone)]
pub struct FunctionBoundary {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Chunk a unified diff by function boundaries.
/// Each hunk's changes are attributed to the enclosing function.
pub fn chunk_diff_by_functions(
    diff: &UnifiedDiff,
    file_content: Option<&str>,
) -> Vec<FunctionChunk> {
    let language = detect_language(&diff.file_path);
    let boundaries = if let Some(content) = file_content {
        detect_function_boundaries(content, &language)
    } else {
        // Fall back to hunk context lines for function detection
        detect_boundaries_from_hunks(&diff.hunks, &language)
    };

    if boundaries.is_empty() {
        // If no functions detected, return whole-file chunks per hunk
        return diff
            .hunks
            .iter()
            .map(|hunk| {
                let (added, removed, context) = count_changes(&hunk.changes);
                FunctionChunk {
                    function_name: "<top-level>".to_string(),
                    file_path: diff.file_path.clone(),
                    start_line: hunk.new_start,
                    end_line: hunk.new_start + hunk.new_lines.saturating_sub(1),
                    language: language.clone(),
                    changes: hunk.changes.clone(),
                    added_lines: added,
                    removed_lines: removed,
                    context_lines: context,
                }
            })
            .collect();
    }

    let mut chunks: Vec<FunctionChunk> = Vec::new();
    let mut orphan_changes: Vec<DiffLine> = Vec::new();

    for hunk in &diff.hunks {
        for change in &hunk.changes {
            let line_no = change.new_line_no.or(change.old_line_no).unwrap_or(0);

            if let Some(func) = find_enclosing_function(&boundaries, line_no) {
                // Find or create chunk for this function
                let chunk = chunks
                    .iter_mut()
                    .find(|c| c.function_name == func.name && c.start_line == func.start_line);

                if let Some(chunk) = chunk {
                    update_chunk_counts(chunk, &change.change_type);
                    chunk.changes.push(change.clone());
                    if line_no > chunk.end_line {
                        chunk.end_line = line_no;
                    }
                } else {
                    let (added, removed, context) = count_single_change(&change.change_type);
                    chunks.push(FunctionChunk {
                        function_name: func.name.clone(),
                        file_path: diff.file_path.clone(),
                        start_line: func.start_line,
                        end_line: func.end_line,
                        language: language.clone(),
                        changes: vec![change.clone()],
                        added_lines: added,
                        removed_lines: removed,
                        context_lines: context,
                    });
                }
            } else {
                orphan_changes.push(change.clone());
            }
        }
    }

    // Add orphan changes as a top-level chunk
    if !orphan_changes.is_empty() {
        let (added, removed, context) = count_changes(&orphan_changes);
        let min_line = orphan_changes
            .iter()
            .filter_map(|c| c.new_line_no.or(c.old_line_no))
            .min()
            .unwrap_or(0);
        let max_line = orphan_changes
            .iter()
            .filter_map(|c| c.new_line_no.or(c.old_line_no))
            .max()
            .unwrap_or(0);

        chunks.push(FunctionChunk {
            function_name: "<top-level>".to_string(),
            file_path: diff.file_path.clone(),
            start_line: min_line,
            end_line: max_line,
            language: language.clone(),
            changes: orphan_changes,
            added_lines: added,
            removed_lines: removed,
            context_lines: context,
        });
    }

    // Sort by start line
    chunks.sort_by_key(|c| c.start_line);
    chunks
}

/// Detect function boundaries in source code using regex patterns.
pub fn detect_function_boundaries(content: &str, language: &str) -> Vec<FunctionBoundary> {
    let pattern = match language {
        "rs" => &*RUST_FN_BOUNDARY,
        "py" => &*PY_FN_BOUNDARY,
        "js" | "jsx" | "ts" | "tsx" => &*JS_FN_BOUNDARY,
        "go" => &*GO_FN_BOUNDARY,
        "java" | "kt" | "cs" => &*JAVA_FN_BOUNDARY,
        _ => return Vec::new(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut boundaries = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if let Some(caps) = pattern.captures(line) {
            if let Some(name) = caps.get(1) {
                let fn_name = name.as_str().to_string();
                if fn_name.len() < 2 {
                    continue;
                }
                let end = find_function_end(&lines, idx, language);
                boundaries.push(FunctionBoundary {
                    name: fn_name,
                    start_line: idx + 1,
                    end_line: end + 1,
                });
            }
        }
    }

    boundaries
}

fn detect_boundaries_from_hunks(hunks: &[DiffHunk], language: &str) -> Vec<FunctionBoundary> {
    let pattern = match language {
        "rs" => &*RUST_FN_BOUNDARY,
        "py" => &*PY_FN_BOUNDARY,
        "js" | "jsx" | "ts" | "tsx" => &*JS_FN_BOUNDARY,
        "go" => &*GO_FN_BOUNDARY,
        "java" | "kt" | "cs" => &*JAVA_FN_BOUNDARY,
        _ => return Vec::new(),
    };

    let mut boundaries = Vec::new();

    for hunk in hunks {
        // Check hunk context line (often contains function name)
        if let Some(caps) = pattern.captures(&hunk.context) {
            if let Some(name) = caps.get(1) {
                boundaries.push(FunctionBoundary {
                    name: name.as_str().to_string(),
                    start_line: hunk.new_start,
                    end_line: hunk.new_start + hunk.new_lines.saturating_sub(1),
                });
            }
        }

        // Also check the change lines themselves
        for change in &hunk.changes {
            if let Some(caps) = pattern.captures(&change.content) {
                if let Some(name) = caps.get(1) {
                    let line = change.new_line_no.or(change.old_line_no).unwrap_or(0);
                    boundaries.push(FunctionBoundary {
                        name: name.as_str().to_string(),
                        start_line: line,
                        end_line: line + 20, // estimate
                    });
                }
            }
        }
    }

    boundaries
}

fn find_enclosing_function(
    boundaries: &[FunctionBoundary],
    line: usize,
) -> Option<&FunctionBoundary> {
    // Find the innermost function that contains this line
    boundaries
        .iter()
        .filter(|b| line >= b.start_line && line <= b.end_line)
        .min_by_key(|b| b.end_line - b.start_line)
}

fn find_function_end(lines: &[&str], start: usize, language: &str) -> usize {
    match language {
        "py" => {
            // Python: use indentation
            let base_indent = lines[start].len() - lines[start].trim_start().len();
            for (i, line) in lines.iter().enumerate().skip(start + 1) {
                if line.trim().is_empty() {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                if indent <= base_indent && !line.trim().is_empty() {
                    return i.saturating_sub(1);
                }
            }
            lines.len().saturating_sub(1)
        }
        _ => {
            // Brace-based languages — skip braces inside string literals
            let mut depth = 0i32;
            let mut found_open = false;
            for (i, line) in lines.iter().enumerate().skip(start) {
                let mut in_double_quote = false;
                let mut in_single_quote = false;
                let mut escaped = false;
                for ch in line.chars() {
                    if escaped {
                        escaped = false;
                    } else if (in_double_quote || in_single_quote) && ch == '\\' {
                        escaped = true;
                    } else if ch == '"' && !in_single_quote {
                        in_double_quote = !in_double_quote;
                    } else if ch == '\'' && !in_double_quote && language != "rs" {
                        in_single_quote = !in_single_quote;
                    } else if !in_double_quote && !in_single_quote {
                        if ch == '{' {
                            depth += 1;
                            found_open = true;
                        } else if ch == '}' {
                            depth -= 1;
                        }
                    }
                }
                if found_open && depth <= 0 {
                    return i;
                }
            }
            lines.len().saturating_sub(1)
        }
    }
}

/// Given file content and a line number, find the start line of the enclosing
/// function/class boundary. Searches upward from `line` up to `max_search_lines`.
/// Returns the boundary start line, or None if no boundary found.
pub fn find_enclosing_boundary_line(
    content: &str,
    file_path: &Path,
    line: usize,
    _max_search_lines: usize,
) -> Option<usize> {
    let language = detect_language(file_path);
    let boundaries = detect_function_boundaries(content, &language);
    // Find the innermost function containing this line
    find_enclosing_function(&boundaries, line).map(|b| b.start_line)
}

fn detect_language(file_path: &Path) -> String {
    file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string()
}

fn count_changes(changes: &[DiffLine]) -> (usize, usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    let mut context = 0;
    for c in changes {
        match c.change_type {
            ChangeType::Added => added += 1,
            ChangeType::Removed => removed += 1,
            ChangeType::Context => context += 1,
        }
    }
    (added, removed, context)
}

fn count_single_change(change_type: &ChangeType) -> (usize, usize, usize) {
    match change_type {
        ChangeType::Added => (1, 0, 0),
        ChangeType::Removed => (0, 1, 0),
        ChangeType::Context => (0, 0, 1),
    }
}

fn update_chunk_counts(chunk: &mut FunctionChunk, change_type: &ChangeType) {
    match change_type {
        ChangeType::Added => chunk.added_lines += 1,
        ChangeType::Removed => chunk.removed_lines += 1,
        ChangeType::Context => chunk.context_lines += 1,
    }
}

static RUST_FN_BOUNDARY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\s*(?:pub(?:\(crate\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+(?:"[^"]*"\s+)?)?fn\s+([A-Za-z_]\w*)"#).unwrap()
});
static PY_FN_BOUNDARY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)").unwrap());
static JS_FN_BOUNDARY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$]\w*)").unwrap());
static GO_FN_BOUNDARY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)").unwrap());
static JAVA_FN_BOUNDARY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:public|private|protected)?\s*(?:static\s+)?(?:\w+\s+)+([A-Za-z_]\w*)\s*\(")
        .unwrap()
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_diff_line(line_no: usize, change_type: ChangeType, content: &str) -> DiffLine {
        match change_type {
            ChangeType::Added => DiffLine {
                old_line_no: None,
                new_line_no: Some(line_no),
                change_type,
                content: content.to_string(),
            },
            ChangeType::Removed => DiffLine {
                old_line_no: Some(line_no),
                new_line_no: None,
                change_type,
                content: content.to_string(),
            },
            ChangeType::Context => DiffLine {
                old_line_no: Some(line_no),
                new_line_no: Some(line_no),
                change_type,
                content: content.to_string(),
            },
        }
    }

    #[test]
    fn test_detect_rust_function_boundaries() {
        let content = r#"
use std::io;

pub fn alpha(x: i32) -> bool {
    x > 0
}

fn beta() {
    println!("hello");
}

pub async fn gamma() -> Result<()> {
    Ok(())
}
"#;
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 3);
        assert_eq!(boundaries[0].name, "alpha");
        assert_eq!(boundaries[1].name, "beta");
        assert_eq!(boundaries[2].name, "gamma");
    }

    #[test]
    fn test_detect_python_function_boundaries() {
        let content = "def foo():\n    return 1\n\ndef bar(x):\n    if x:\n        return x\n    return 0\n\nclass Baz:\n    pass\n";
        let boundaries = detect_function_boundaries(content, "py");
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].name, "foo");
        assert_eq!(boundaries[1].name, "bar");
    }

    #[test]
    fn test_detect_js_function_boundaries() {
        let content = "function render() {\n  return null;\n}\n\nexport async function fetchData() {\n  const resp = await fetch(url);\n}\n";
        let boundaries = detect_function_boundaries(content, "js");
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].name, "render");
        assert_eq!(boundaries[1].name, "fetchData");
    }

    #[test]
    fn test_detect_go_function_boundaries() {
        let content = "func main() {\n\tfmt.Println(\"hello\")\n}\n\nfunc (s *Server) Start() {\n\ts.listen()\n}\n";
        let boundaries = detect_function_boundaries(content, "go");
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].name, "main");
        assert_eq!(boundaries[1].name, "Start");
    }

    #[test]
    fn test_chunk_diff_by_functions_with_content() {
        let file_content = r#"pub fn alpha() {
    let x = 1;
    let y = 2;
    x + y
}

pub fn beta() {
    println!("hello");
    println!("world");
}
"#;
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 2,
                old_lines: 1,
                new_start: 2,
                new_lines: 1,
                context: "@@ -2,1 +2,1 @@ pub fn alpha()".to_string(),
                changes: vec![
                    make_diff_line(2, ChangeType::Removed, "    let x = 1;"),
                    make_diff_line(2, ChangeType::Added, "    let x = 42;"),
                    make_diff_line(8, ChangeType::Added, "    println!(\"extra\");"),
                ],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, Some(file_content));
        assert!(!chunks.is_empty());

        // Should have at least an alpha chunk
        let alpha_chunk = chunks.iter().find(|c| c.function_name == "alpha");
        assert!(alpha_chunk.is_some());
    }

    #[test]
    fn test_chunk_diff_no_file_content() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                context: "@@ -1,1 +1,1 @@".to_string(),
                changes: vec![make_diff_line(1, ChangeType::Added, "new line")],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, None);
        assert!(!chunks.is_empty());
        // Should be a top-level chunk since no boundaries detected
        assert_eq!(chunks[0].function_name, "<top-level>");
    }

    #[test]
    fn test_function_chunk_metrics() {
        let chunk = FunctionChunk {
            function_name: "test".to_string(),
            file_path: PathBuf::from("test.rs"),
            start_line: 1,
            end_line: 20,
            language: "rs".to_string(),
            changes: vec![],
            added_lines: 5,
            removed_lines: 3,
            context_lines: 12,
        };

        assert_eq!(chunk.total_changes(), 8);
        assert!((chunk.change_density() - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_change_density_zero_span() {
        let chunk = FunctionChunk {
            function_name: "test".to_string(),
            file_path: PathBuf::from("test.rs"),
            start_line: 5,
            end_line: 5,
            language: "rs".to_string(),
            changes: vec![],
            added_lines: 1,
            removed_lines: 0,
            context_lines: 0,
        };

        assert_eq!(chunk.total_changes(), 1);
        assert!((chunk.change_density() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_find_enclosing_function() {
        let boundaries = vec![
            FunctionBoundary {
                name: "outer".to_string(),
                start_line: 1,
                end_line: 50,
            },
            FunctionBoundary {
                name: "inner".to_string(),
                start_line: 10,
                end_line: 30,
            },
        ];

        // Line 15 is inside inner (which is inside outer) - should return inner (most specific)
        let result = find_enclosing_function(&boundaries, 15);
        assert_eq!(result.unwrap().name, "inner");

        // Line 5 is only inside outer
        let result = find_enclosing_function(&boundaries, 5);
        assert_eq!(result.unwrap().name, "outer");

        // Line 55 is outside everything
        let result = find_enclosing_function(&boundaries, 55);
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_diff_produces_no_chunks() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, None);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_multiple_hunks_same_function() {
        let file_content = "pub fn big_function() {\n    line1;\n    line2;\n    line3;\n    line4;\n    line5;\n    line6;\n    line7;\n    line8;\n    line9;\n}\n";
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![
                DiffHunk {
                    old_start: 2,
                    old_lines: 1,
                    new_start: 2,
                    new_lines: 1,
                    context: "@@ -2,1 +2,1 @@".to_string(),
                    changes: vec![make_diff_line(2, ChangeType::Added, "    changed1;")],
                },
                DiffHunk {
                    old_start: 8,
                    old_lines: 1,
                    new_start: 8,
                    new_lines: 1,
                    context: "@@ -8,1 +8,1 @@".to_string(),
                    changes: vec![make_diff_line(8, ChangeType::Added, "    changed2;")],
                },
            ],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, Some(file_content));
        // Both hunks should be in the same function chunk
        let fn_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.function_name == "big_function")
            .collect();
        assert_eq!(fn_chunks.len(), 1);
        assert_eq!(fn_chunks[0].added_lines, 2);
    }

    #[test]
    fn test_chunk_file_path_and_language() {
        let file_content = "pub fn hello() {\n    println!(\"hi\");\n}\n";
        let diff = UnifiedDiff {
            file_path: PathBuf::from("src/greeter.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 2,
                old_lines: 1,
                new_start: 2,
                new_lines: 1,
                context: "@@ -2,1 +2,1 @@ pub fn hello()".to_string(),
                changes: vec![make_diff_line(
                    2,
                    ChangeType::Added,
                    "    println!(\"hello\");",
                )],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, Some(file_content));
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].file_path, PathBuf::from("src/greeter.rs"));
        assert_eq!(chunks[0].language, "rs");
    }

    #[test]
    fn test_unsupported_language_returns_toplevel() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.txt"),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                context: "@@ -1,1 +1,1 @@".to_string(),
                changes: vec![make_diff_line(1, ChangeType::Added, "new text")],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let chunks = chunk_diff_by_functions(&diff, Some("some text\n"));
        assert_eq!(chunks[0].function_name, "<top-level>");
    }

    #[test]
    fn test_empty_diff_no_hunks() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("empty.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };
        let chunks = chunk_diff_by_functions(&diff, None);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_change_density_single_line() {
        let chunk = FunctionChunk {
            function_name: "f".to_string(),
            file_path: PathBuf::from("t.rs"),
            start_line: 5,
            end_line: 5,
            language: "rs".to_string(),
            changes: vec![],
            added_lines: 1,
            removed_lines: 0,
            context_lines: 0,
        };
        // span = 1, total_changes = 1
        assert!((chunk.change_density() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_rust_const_fn_detection() {
        let content = "pub const fn max_value() -> u32 {\n    u32::MAX\n}\n";
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].name, "max_value");
    }

    #[test]
    fn test_rust_unsafe_fn_detection() {
        let content = "pub unsafe fn dangerous() {\n    // ptr stuff\n}\n";
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].name, "dangerous");
    }

    #[test]
    fn test_rust_async_fn_detection() {
        let content = "pub async fn fetch_data() -> Result<()> {\n    Ok(())\n}\n";
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].name, "fetch_data");
    }

    #[test]
    fn test_rust_pub_crate_fn_detection() {
        let content = "pub(crate) fn internal_helper() {\n    // ...\n}\n";
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].name, "internal_helper");
    }

    #[test]
    fn test_chunk_diff_end_line_correctness() {
        // A hunk starting at line 10 with 3 lines should end at line 12, not 13
        let diff = UnifiedDiff {
            file_path: PathBuf::from("test.txt"),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 10,
                old_lines: 3,
                new_start: 10,
                new_lines: 3,
                context: "@@ -10,3 +10,3 @@".to_string(),
                changes: vec![
                    make_diff_line(10, ChangeType::Context, "line 10"),
                    make_diff_line(11, ChangeType::Added, "line 11"),
                    make_diff_line(12, ChangeType::Context, "line 12"),
                ],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };
        let chunks = chunk_diff_by_functions(&diff, None);
        assert_eq!(chunks[0].start_line, 10);
        assert_eq!(chunks[0].end_line, 12); // not 13
    }

    #[test]
    fn test_multiple_functions_in_diff() {
        let content = r#"fn first() {
    println!("a");
}

fn second() {
    println!("b");
}
"#;
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].name, "first");
        assert_eq!(boundaries[1].name, "second");
    }

    #[test]
    fn test_go_method_detection() {
        let content =
            "func (s *Server) handleRequest(w http.ResponseWriter, r *http.Request) {\n}\n";
        let boundaries = detect_function_boundaries(content, "go");
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].name, "handleRequest");
    }

    // Regression: brace counting must skip braces inside string literals
    #[test]
    fn test_function_end_unbalanced_string_braces() {
        let content = r#"fn render() {
    let open_brace = "{{{{";
    let msg = format!("value: {}", x);
    println!("done");
}

fn next_func() {
    // should be separate
}
"#;
        let boundaries = detect_function_boundaries(content, "rs");
        assert_eq!(
            boundaries.len(),
            2,
            "Should find both functions: {:?}",
            boundaries
        );
        // render() should end at the } on line 5 (before next_func)
        assert!(
            boundaries[0].end_line < boundaries[1].start_line,
            "render ends at {} but next_func starts at {}",
            boundaries[0].end_line,
            boundaries[1].start_line
        );
    }

    // Regression: Python function end detection must work for methods inside classes
    #[test]
    fn test_python_method_boundaries() {
        let content = r#"class Server:
    def handle_request(self, request):
        response = self.process(request)
        return response

    def process(self, request):
        return request.upper()
"#;
        let boundaries = detect_function_boundaries(content, "py");
        assert_eq!(
            boundaries.len(),
            2,
            "Should find both methods: {:?}",
            boundaries
        );
        assert_eq!(boundaries[0].name, "handle_request");
        assert_eq!(boundaries[1].name, "process");
        // handle_request should end before process starts
        assert!(
            boundaries[0].end_line < boundaries[1].start_line,
            "handle_request ends at {} but process starts at {}",
            boundaries[0].end_line,
            boundaries[1].start_line
        );
    }

    // find_enclosing_function: verify no underflow if end_line < start_line
    #[test]
    fn test_find_enclosing_function_bad_boundary() {
        let boundaries = vec![
            FunctionBoundary {
                name: "broken".to_string(),
                start_line: 10,
                end_line: 5, // invalid: end < start
            },
            FunctionBoundary {
                name: "valid".to_string(),
                start_line: 1,
                end_line: 20,
            },
        ];
        // Line 7: should match "valid" (1-20) but NOT "broken" (10-5)
        // The min_by_key uses end_line - start_line, which would underflow for "broken"
        // if it weren't filtered out first. But if both match filter, it WOULD panic.
        let result = find_enclosing_function(&boundaries, 12);
        // Line 12: matches "valid" (1-20). Should NOT panic even if "broken" passes filter.
        assert!(result.is_some());
    }

    #[test]
    fn test_find_enclosing_boundary_line_rust() {
        let content =
            "use std::io;\n\npub fn process(x: i32) -> bool {\n    let y = x + 1;\n    y > 0\n}\n";
        let path = Path::new("test.rs");
        // Line 4 is inside process() which starts at line 3
        let result = find_enclosing_boundary_line(content, path, 4, 10);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_find_enclosing_boundary_line_no_function() {
        let content = "let x = 1;\nlet y = 2;\n";
        let path = Path::new("test.rs");
        let result = find_enclosing_boundary_line(content, path, 1, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_enclosing_boundary_line_python() {
        let content =
            "import os\n\ndef handle_request(req):\n    data = req.json()\n    return data\n";
        let path = Path::new("handler.py");
        // Line 4 is inside handle_request() which starts at line 3
        let result = find_enclosing_boundary_line(content, path, 4, 10);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_find_enclosing_boundary_line_unsupported_language() {
        let content = "some text\nmore text\n";
        let path = Path::new("readme.txt");
        let result = find_enclosing_boundary_line(content, path, 1, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_enclosing_boundary_nested_functions() {
        let content = "pub fn outer() {\n    fn inner() {\n        let x = 1;\n    }\n}\n";
        let path = Path::new("test.rs");
        // Line 3 is inside inner() which starts at line 2
        let result = find_enclosing_boundary_line(content, path, 3, 10);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_find_function_end_single_quoted_string_with_brace() {
        // BUG: single-quoted strings aren't tracked, so `'{'` is counted as a real brace
        let lines: Vec<&str> = vec![
            "function foo() {",         // line 0: depth = 1
            "    let s = '{';",         // line 1: depth should stay 1, but goes to 2
            "    console.log(s);",      // line 2
            "}",                        // line 3: depth goes to 1 (not 0!), so foo "never ends"
            "",                         // line 4
            "function bar() {",         // line 5: depth goes to 2
            "    return 1;",            // line 6
            "}",                        // line 7: depth goes to 1, and foo is "found" here (wrong!)
        ];
        let end = find_function_end(&lines, 0, "js");
        assert_eq!(
            end, 3,
            "foo() should end at line 3, not bleed into bar() due to untracked single-quoted brace"
        );
    }

    #[test]
    fn test_find_function_end_rust_lifetime_not_string() {
        // BUG: Rust lifetime annotations ('a, 'static) toggle in_single_quote,
        // causing the opening brace on the same line to be ignored.
        let lines: Vec<&str> = vec![
            "pub fn new(name: &'static str) -> Self {", // line 0: has lifetime '
            "    Self { name }",                        // line 1
            "}",                                        // line 2
            "",
            "fn other() {",                             // line 4
        ];
        let end = find_function_end(&lines, 0, "rs");
        assert_eq!(
            end, 2,
            "Rust lifetime annotation should not break brace tracking"
        );
    }

    #[test]
    fn test_find_function_end_escaped_backslash_in_string() {
        // A string containing "\\" (escaped backslash) on the same line as braces.
        // The closing quote after "\\" terminates the string, so the closing brace
        // on the same line should be counted.
        let lines: Vec<&str> = vec![
            r#"function foo() { let s = "\\"; }"#, // line 0: open+close on same line
            "function next() {",                   // line 1: next function
        ];
        let end = find_function_end(&lines, 0, "js");
        assert_eq!(
            end, 0,
            "Function should end at line 0 (braces balanced on same line with escaped backslash)"
        );
    }
}
