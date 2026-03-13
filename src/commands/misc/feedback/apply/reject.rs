use crate::core;
use crate::review;

use super::stats::record_rejected_feedback;

pub(in super::super) fn apply_feedback_reject(
    store: &mut review::FeedbackStore,
    comments: &[core::Comment],
) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.suppress.insert(comment.id.clone());
        if is_new {
            updated += 1;
            record_rejected_feedback(store, comment);
        }
        store.accept.remove(&comment.id);
    }
    updated
}
