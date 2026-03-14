use crate::core;

pub(super) fn merge_specialized_comment(existing: &mut core::Comment, comment: core::Comment) {
    if comment.confidence > existing.confidence {
        existing.content = comment.content.clone();
        existing.confidence = comment.confidence;
        existing.severity = comment.severity.clone();
    }

    if existing.rule_id.is_none() {
        existing.rule_id = comment.rule_id.clone();
    }
    if existing.suggestion.is_none() {
        existing.suggestion = comment.suggestion.clone();
    }
    if existing.code_suggestion.is_none() {
        existing.code_suggestion = comment.code_suggestion.clone();
    }

    for tag in &comment.tags {
        if !existing.tags.contains(tag) {
            existing.tags.push(tag.clone());
        }
    }
}
