use crate::core::diff_parser::UnifiedDiff;

use super::changes::{
    collect_non_context_changes, is_deletion_only_change, is_whitespace_only_change,
};
use super::comments::is_comment_line;
use super::files::{is_generated_file, is_lock_file};
use super::result::TriageResult;

/// Options for triage behavior (e.g. from config).
#[derive(Debug, Clone, Copy, Default)]
pub struct TriageOptions {
    /// When true, treat deletion-only diffs as skip (#29). Default false (deletions still get review).
    pub skip_deletion_only: bool,
}

/// Convenience: triage with default options (skip_deletion_only = false). Used by tests and callers that do not need config.
#[allow(dead_code)]
pub fn triage_diff(diff: &UnifiedDiff) -> TriageResult {
    triage_diff_with_options(diff, TriageOptions::default())
}

pub fn triage_diff_with_options(diff: &UnifiedDiff, options: TriageOptions) -> TriageResult {
    if is_lock_file(&diff.file_path) {
        return TriageResult::SkipLockFile;
    }

    let path_str = diff.file_path.to_string_lossy();
    if is_generated_file(&path_str) {
        return TriageResult::SkipGenerated;
    }

    let all_changes = collect_non_context_changes(diff);
    if all_changes.is_empty() {
        return TriageResult::NeedsReview;
    }

    if is_whitespace_only_change(&all_changes) {
        return TriageResult::SkipWhitespaceOnly;
    }

    if all_changes
        .iter()
        .all(|change| is_comment_line(&change.content))
    {
        return TriageResult::SkipCommentOnly;
    }

    if is_deletion_only_change(&all_changes) {
        if options.skip_deletion_only {
            return TriageResult::SkipDeletionOnly;
        }
        // Pure deletions can still remove required fields, checks, or error handling.
        return TriageResult::NeedsReview;
    }

    TriageResult::NeedsReview
}
