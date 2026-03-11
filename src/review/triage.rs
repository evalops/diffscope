use crate::core::diff_parser::UnifiedDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageResult {
    NeedsReview,
    SkipLockFile,
    SkipWhitespaceOnly,
    SkipDeletionOnly,
    SkipGenerated,
    SkipCommentOnly,
}

impl TriageResult {
    pub fn should_skip(&self) -> bool {
        !matches!(self, TriageResult::NeedsReview)
    }

    pub fn reason(&self) -> &'static str {
        match self {
            TriageResult::NeedsReview => "needs review",
            TriageResult::SkipLockFile => "lock file",
            TriageResult::SkipWhitespaceOnly => "whitespace-only changes",
            TriageResult::SkipDeletionOnly => "deletion-only changes",
            TriageResult::SkipGenerated => "generated file",
            TriageResult::SkipCommentOnly => "comment-only changes",
        }
    }
}

/// Lock file names that should be auto-skipped.
const LOCK_FILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Gemfile.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
    "Pipfile.lock",
];

/// Comment line prefixes (after trimming leading whitespace).
const COMMENT_PREFIXES: &[&str] = &["//", "#", "/*", "*/", "* ", "--", "<!--", "\"\"\"", "'''"];

/// Classify a diff using fast heuristics (no LLM call).
///
/// Checks are applied in priority order:
/// 1. Lock files
/// 2. Generated files
/// 3. Deletion-only changes
/// 4. Whitespace-only changes
/// 5. Comment-only changes
/// 6. Default → NeedsReview
pub fn triage_diff(diff: &UnifiedDiff) -> TriageResult {
    let path_str = diff.file_path.to_string_lossy();

    // 1. Lock files — match by file name (final component)
    if let Some(file_name) = diff.file_path.file_name().and_then(|n| n.to_str()) {
        if LOCK_FILES.contains(&file_name) {
            return TriageResult::SkipLockFile;
        }
    }

    // 2. Generated files — match by path patterns and extensions
    if is_generated_file(&path_str) {
        return TriageResult::SkipGenerated;
    }

    // Collect all non-context changes across all hunks
    let all_changes: Vec<&DiffLine> = diff
        .hunks
        .iter()
        .flat_map(|h| h.changes.iter())
        .filter(|c| !matches!(c.change_type, ChangeType::Context))
        .collect();

    // If there are no actual changes, default to NeedsReview
    if all_changes.is_empty() {
        return TriageResult::NeedsReview;
    }

    // 3. Deletion-only — all non-context lines are Removed
    if all_changes
        .iter()
        .all(|c| matches!(c.change_type, ChangeType::Removed))
    {
        return TriageResult::SkipDeletionOnly;
    }

    // 4. Whitespace-only — every added line has a corresponding removed line
    //    that differs only in whitespace
    if is_whitespace_only_change(&all_changes) {
        return TriageResult::SkipWhitespaceOnly;
    }

    // 5. Comment-only — all changed lines are comment lines
    if all_changes.iter().all(|c| is_comment_line(&c.content)) {
        return TriageResult::SkipCommentOnly;
    }

    // 6. Default
    TriageResult::NeedsReview
}

/// Check if a file path matches generated-file patterns.
fn is_generated_file(path: &str) -> bool {
    // Path contains marker segments
    if path.contains(".generated.")
        || path.contains(".g.")
        || path.contains("_generated/")
        || path.contains("generated/")
    {
        return true;
    }

    // File extension patterns
    if path.ends_with(".pb.go")
        || path.ends_with(".pb.rs")
        || path.ends_with(".swagger.json")
        || path.ends_with(".min.js")
        || path.ends_with(".min.css")
    {
        return true;
    }

    // Vendor prefix
    if path.starts_with("vendor/") {
        return true;
    }

    false
}

/// Check if all changes are whitespace-only by comparing stripped content
/// of removed vs added lines.
fn is_whitespace_only_change(changes: &[&DiffLine]) -> bool {
    let removed: Vec<&str> = changes
        .iter()
        .filter(|c| matches!(c.change_type, ChangeType::Removed))
        .map(|c| c.content.as_str())
        .collect();

    let added: Vec<&str> = changes
        .iter()
        .filter(|c| matches!(c.change_type, ChangeType::Added))
        .map(|c| c.content.as_str())
        .collect();

    // Must have the same number of added and removed lines
    if removed.len() != added.len() {
        return false;
    }

    // Each pair must differ only in whitespace
    removed
        .iter()
        .zip(added.iter())
        .all(|(r, a)| strip_whitespace(r) == strip_whitespace(a))
}

/// Remove all whitespace characters from a string for comparison.
fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Check if a line (after trimming leading whitespace) starts with a comment prefix.
fn is_comment_line(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        // Blank lines in a comment-only change are fine
        return true;
    }
    COMMENT_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

use crate::core::diff_parser::{ChangeType, DiffLine};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine, UnifiedDiff};
    use std::path::PathBuf;

    /// Helper to build a test diff with a single hunk containing the given changes.
    fn make_diff(file_path: &str, changes: Vec<DiffLine>) -> UnifiedDiff {
        let hunks = if changes.is_empty() {
            vec![]
        } else {
            vec![DiffHunk {
                old_start: 1,
                old_lines: changes
                    .iter()
                    .filter(|c| matches!(c.change_type, ChangeType::Removed | ChangeType::Context))
                    .count(),
                new_start: 1,
                new_lines: changes
                    .iter()
                    .filter(|c| matches!(c.change_type, ChangeType::Added | ChangeType::Context))
                    .count(),
                context: String::new(),
                changes,
            }]
        };
        UnifiedDiff {
            file_path: PathBuf::from(file_path),
            old_content: None,
            new_content: None,
            hunks,
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    fn make_line(line_no: usize, change_type: ChangeType, content: &str) -> DiffLine {
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

    // ── Lock file tests ──────────────────────────────────────────────────

    #[test]
    fn test_lock_file_cargo_lock() {
        let diff = make_diff(
            "Cargo.lock",
            vec![make_line(1, ChangeType::Added, "some dep change")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_package_lock_json() {
        let diff = make_diff(
            "package-lock.json",
            vec![make_line(1, ChangeType::Added, "some dep change")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_yarn_lock() {
        let diff = make_diff(
            "yarn.lock",
            vec![make_line(1, ChangeType::Added, "resolved version")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_go_sum() {
        let diff = make_diff(
            "go.sum",
            vec![make_line(
                1,
                ChangeType::Added,
                "golang.org/x/text v0.3.7 h1:abc",
            )],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_pnpm_lock() {
        let diff = make_diff(
            "pnpm-lock.yaml",
            vec![make_line(1, ChangeType::Added, "lockfileVersion: '6.0'")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_gemfile_lock() {
        let diff = make_diff("Gemfile.lock", vec![make_line(1, ChangeType::Added, "GEM")]);
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_poetry_lock() {
        let diff = make_diff(
            "poetry.lock",
            vec![make_line(1, ChangeType::Added, "[[package]]")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_composer_lock() {
        let diff = make_diff("composer.lock", vec![make_line(1, ChangeType::Added, "{}")]);
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_lock_file_pipfile_lock() {
        let diff = make_diff("Pipfile.lock", vec![make_line(1, ChangeType::Added, "{}")]);
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_non_lock_file_passes() {
        let diff = make_diff(
            "Cargo.toml",
            vec![
                make_line(1, ChangeType::Removed, "version = \"0.1.0\""),
                make_line(1, ChangeType::Added, "version = \"0.2.0\""),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    // ── Generated file tests ─────────────────────────────────────────────

    #[test]
    fn test_generated_pb_go() {
        let diff = make_diff(
            "proto/service.pb.go",
            vec![make_line(
                1,
                ChangeType::Added,
                "// Code generated by protoc",
            )],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_pb_rs() {
        let diff = make_diff(
            "proto/service.pb.rs",
            vec![make_line(1, ChangeType::Added, "// Generated code")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_vendor_prefix() {
        let diff = make_diff(
            "vendor/github.com/lib/pq/conn.go",
            vec![make_line(1, ChangeType::Added, "package pq")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_min_js() {
        let diff = make_diff(
            "assets/bundle.min.js",
            vec![make_line(1, ChangeType::Added, "!function(e){}")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_min_css() {
        let diff = make_diff(
            "assets/style.min.css",
            vec![make_line(1, ChangeType::Added, "body{margin:0}")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_swagger_json() {
        let diff = make_diff(
            "api/swagger.swagger.json",
            vec![make_line(1, ChangeType::Added, "{\"swagger\":\"2.0\"}")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_path_contains_generated() {
        let diff = make_diff(
            "src/generated/types.ts",
            vec![make_line(1, ChangeType::Added, "export interface Foo {}")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_path_contains_dot_generated() {
        let diff = make_diff(
            "src/schema.generated.ts",
            vec![make_line(1, ChangeType::Added, "export type Bar = string")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_path_contains_dot_g_dot() {
        let diff = make_diff(
            "lib/model.g.dart",
            vec![make_line(1, ChangeType::Added, "part of 'model.dart';")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_generated_underscore_generated_dir() {
        let diff = make_diff(
            "src/_generated/client.ts",
            vec![make_line(1, ChangeType::Added, "export class Client {}")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_non_generated_passes() {
        let diff = make_diff(
            "src/main.rs",
            vec![
                make_line(1, ChangeType::Removed, "let x = 1;"),
                make_line(1, ChangeType::Added, "let x = 2;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    // ── Deletion-only tests ──────────────────────────────────────────────

    #[test]
    fn test_deletion_only_all_removed() {
        let diff = make_diff(
            "src/old_module.rs",
            vec![
                make_line(1, ChangeType::Removed, "fn old_function() {}"),
                make_line(2, ChangeType::Removed, "fn another_old() {}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipDeletionOnly);
    }

    #[test]
    fn test_deletion_only_with_context_lines() {
        let diff = make_diff(
            "src/module.rs",
            vec![
                make_line(1, ChangeType::Context, "use std::io;"),
                make_line(2, ChangeType::Removed, "fn old_function() {}"),
                make_line(3, ChangeType::Context, "fn keep() {}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipDeletionOnly);
    }

    #[test]
    fn test_mixed_changes_not_deletion_only() {
        let diff = make_diff(
            "src/module.rs",
            vec![
                make_line(1, ChangeType::Removed, "fn old() {}"),
                make_line(1, ChangeType::Added, "fn new_version() {}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    // ── Whitespace-only tests ────────────────────────────────────────────

    #[test]
    fn test_whitespace_only_changes() {
        let diff = make_diff(
            "src/formatting.rs",
            vec![
                make_line(1, ChangeType::Removed, "let x = 1;"),
                make_line(1, ChangeType::Added, "  let x = 1;"),
                make_line(2, ChangeType::Removed, "  let y = 2;"),
                make_line(2, ChangeType::Added, "    let y = 2;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipWhitespaceOnly);
    }

    #[test]
    fn test_whitespace_only_tabs_vs_spaces() {
        let diff = make_diff(
            "src/indent.rs",
            vec![
                make_line(1, ChangeType::Removed, "\tlet x = 1;"),
                make_line(1, ChangeType::Added, "    let x = 1;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipWhitespaceOnly);
    }

    #[test]
    fn test_whitespace_only_trailing_spaces() {
        let diff = make_diff(
            "src/trailing.rs",
            vec![
                make_line(1, ChangeType::Removed, "let x = 1;   "),
                make_line(1, ChangeType::Added, "let x = 1;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipWhitespaceOnly);
    }

    #[test]
    fn test_non_whitespace_changes() {
        let diff = make_diff(
            "src/real.rs",
            vec![
                make_line(1, ChangeType::Removed, "let x = 1;"),
                make_line(1, ChangeType::Added, "let x = 2;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    // ── Comment-only tests ───────────────────────────────────────────────

    #[test]
    fn test_comment_only_rust_comments() {
        let diff = make_diff(
            "src/lib.rs",
            vec![
                make_line(1, ChangeType::Removed, "// old comment"),
                make_line(1, ChangeType::Added, "// new comment"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_added_comments() {
        let diff = make_diff(
            "src/lib.rs",
            vec![
                make_line(1, ChangeType::Added, "// a new comment"),
                make_line(2, ChangeType::Added, "// another new comment"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_python_comments() {
        let diff = make_diff(
            "src/app.py",
            vec![
                make_line(1, ChangeType::Removed, "# old python comment"),
                make_line(1, ChangeType::Added, "# new python comment"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_c_style_block_comments() {
        let diff = make_diff(
            "src/main.c",
            vec![
                make_line(1, ChangeType::Added, "/* new block comment */"),
                make_line(2, ChangeType::Added, " * continuation line"),
                make_line(3, ChangeType::Added, " */"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_sql_comments() {
        let diff = make_diff(
            "migrations/001.sql",
            vec![
                make_line(1, ChangeType::Removed, "-- old sql comment"),
                make_line(1, ChangeType::Added, "-- new sql comment"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_html_comments() {
        let diff = make_diff(
            "index.html",
            vec![make_line(1, ChangeType::Added, "<!-- new html comment -->")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipCommentOnly);
    }

    #[test]
    fn test_comment_only_python_docstring() {
        let diff = make_diff(
            "src/app.py",
            vec![
                make_line(1, ChangeType::Added, "\"\"\""),
                make_line(2, ChangeType::Added, "New docstring"),
                // Note: the middle line is not a comment prefix, so this should NOT be SkipCommentOnly.
            ],
        );
        // The middle line "New docstring" doesn't start with a comment prefix,
        // so this should be NeedsReview.
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    #[test]
    fn test_comment_only_mixed_with_code() {
        let diff = make_diff(
            "src/lib.rs",
            vec![
                make_line(1, ChangeType::Removed, "// old comment"),
                make_line(1, ChangeType::Added, "let x = 42;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    #[test]
    fn test_comment_only_does_not_match_pointer_dereference_code() {
        let diff = make_diff(
            "src/lib.rs",
            vec![
                make_line(1, ChangeType::Added, "*ptr = compute();"),
                make_line(2, ChangeType::Added, "*buffer = data;"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn test_empty_hunks_needs_review() {
        let diff = make_diff("src/empty.rs", vec![]);
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    #[test]
    fn test_normal_code_change() {
        let diff = make_diff(
            "src/main.rs",
            vec![
                make_line(1, ChangeType::Context, "fn main() {"),
                make_line(2, ChangeType::Removed, "    println!(\"hello\");"),
                make_line(2, ChangeType::Added, "    println!(\"world\");"),
                make_line(3, ChangeType::Context, "}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    #[test]
    fn test_lock_file_in_subdirectory() {
        // Lock files should be detected even in subdirectories
        let diff = make_diff(
            "packages/web/package-lock.json",
            vec![make_line(1, ChangeType::Added, "dep change")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_priority_lock_file_over_deletion() {
        // Lock file check should take priority over deletion-only check
        let diff = make_diff(
            "Cargo.lock",
            vec![make_line(1, ChangeType::Removed, "old dep")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipLockFile);
    }

    #[test]
    fn test_priority_generated_over_deletion() {
        // Generated file check should take priority over deletion-only check
        let diff = make_diff(
            "vendor/lib/code.go",
            vec![make_line(1, ChangeType::Removed, "old code")],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipGenerated);
    }

    #[test]
    fn test_context_only_lines_needs_review() {
        // A diff with only context lines (no actual changes) should be NeedsReview
        let diff = make_diff(
            "src/lib.rs",
            vec![
                make_line(1, ChangeType::Context, "fn main() {"),
                make_line(2, ChangeType::Context, "}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }

    #[test]
    fn test_whitespace_only_with_context_lines() {
        let diff = make_diff(
            "src/fmt.rs",
            vec![
                make_line(1, ChangeType::Context, "fn foo() {"),
                make_line(2, ChangeType::Removed, "    let x = 1;"),
                make_line(2, ChangeType::Added, "\tlet x = 1;"),
                make_line(3, ChangeType::Context, "}"),
            ],
        );
        assert_eq!(triage_diff(&diff), TriageResult::SkipWhitespaceOnly);
    }

    #[test]
    fn test_multiple_hunks_deletion_only() {
        let mut diff = make_diff(
            "src/cleanup.rs",
            vec![make_line(1, ChangeType::Removed, "fn old1() {}")],
        );
        diff.hunks.push(DiffHunk {
            old_start: 10,
            old_lines: 1,
            new_start: 10,
            new_lines: 0,
            context: String::new(),
            changes: vec![make_line(10, ChangeType::Removed, "fn old2() {}")],
        });
        assert_eq!(triage_diff(&diff), TriageResult::SkipDeletionOnly);
    }

    #[test]
    fn test_multiple_hunks_mixed_not_deletion_only() {
        let mut diff = make_diff(
            "src/mixed.rs",
            vec![make_line(1, ChangeType::Removed, "fn old() {}")],
        );
        diff.hunks.push(DiffHunk {
            old_start: 10,
            old_lines: 0,
            new_start: 10,
            new_lines: 1,
            context: String::new(),
            changes: vec![make_line(10, ChangeType::Added, "fn new_thing() {}")],
        });
        assert_eq!(triage_diff(&diff), TriageResult::NeedsReview);
    }
}
