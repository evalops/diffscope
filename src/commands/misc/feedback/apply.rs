use crate::core;
use crate::review;

pub(super) fn apply_feedback_accept(
    store: &mut review::FeedbackStore,
    comments: &[core::Comment],
) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.accept.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.accepted = stats.accepted.saturating_add(1);
            let file_patterns = review::derive_file_patterns(&comment.file_path);
            store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, true);
        }
        store.suppress.remove(&comment.id);
    }
    updated
}

pub(super) fn apply_feedback_reject(
    store: &mut review::FeedbackStore,
    comments: &[core::Comment],
) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.suppress.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.rejected = stats.rejected.saturating_add(1);
            let file_patterns = review::derive_file_patterns(&comment.file_path);
            store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, false);
        }
        store.accept.remove(&comment.id);
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_feedback_stats_not_double_counted() {
        let mut store = review::FeedbackStore::default();
        let comment = core::Comment {
            id: "cmt_dup".to_string(),
            file_path: PathBuf::from("test.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        };

        let comments = vec![comment];

        for _ in 0..2 {
            apply_feedback_accept(&mut store, &comments);
        }

        let key = review::classify_comment_type(&comments[0])
            .as_str()
            .to_string();
        let stats = &store.by_comment_type[&key];
        assert_eq!(
            stats.accepted, 1,
            "Stats should only count 1 acceptance, not 2 (double-counting bug)"
        );
        assert_eq!(store.by_category["Bug"].accepted, 1);
        assert_eq!(store.by_file_pattern["*.rs"].accepted, 1);
        assert_eq!(store.by_category_file_pattern["Bug|*.rs"].accepted, 1);
    }
}
