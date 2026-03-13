use crate::core;
use crate::review;

pub(super) fn record_accepted_feedback(store: &mut review::FeedbackStore, comment: &core::Comment) {
    record_feedback_stats(store, comment, true);
}

pub(super) fn record_rejected_feedback(store: &mut review::FeedbackStore, comment: &core::Comment) {
    record_feedback_stats(store, comment, false);
}

fn record_feedback_stats(
    store: &mut review::FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
) {
    let key = review::classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    if accepted {
        stats.accepted = stats.accepted.saturating_add(1);
    } else {
        stats.rejected = stats.rejected.saturating_add(1);
    }

    let file_patterns = review::derive_file_patterns(&comment.file_path);
    store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, accepted);
}
