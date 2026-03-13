use crate::core;

pub(super) fn merge_specialized_comment(existing: &mut core::Comment, comment: core::Comment) {
    if comment.confidence > existing.confidence {
        existing.content = comment.content;
        existing.confidence = comment.confidence;
        existing.severity = comment.severity;
    }

    for tag in &comment.tags {
        if !existing.tags.contains(tag) {
            existing.tags.push(tag.clone());
        }
    }
}
