use crate::core;
use crate::review;

pub(in super::super) fn apply_feedback_reject(
    store: &mut review::FeedbackStore,
    comments: &[core::Comment],
) -> usize {
    let mut updated = 0;
    for comment in comments {
        if review::apply_comment_feedback_signal(store, comment, false) {
            updated += 1;
        }
    }
    updated
}
