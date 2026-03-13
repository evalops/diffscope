use crate::core;
use crate::review::filter_comments_for_diff;

pub(super) fn merge_processed_comments(
    diff: &core::UnifiedDiff,
    comments: Vec<core::Comment>,
    deterministic_comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    let mut comments = filter_comments_for_diff(diff, comments);
    comments.extend(deterministic_comments);
    comments
}
