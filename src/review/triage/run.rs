use crate::core::diff_parser::UnifiedDiff;

use super::changes::{
    collect_non_context_changes, is_deletion_only_change, is_whitespace_only_change,
};
use super::comments::is_comment_line;
use super::files::{is_generated_file, is_lock_file};
use super::result::TriageResult;

pub fn triage_diff(diff: &UnifiedDiff) -> TriageResult {
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

    if is_deletion_only_change(&all_changes) {
        return TriageResult::SkipDeletionOnly;
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

    TriageResult::NeedsReview
}
