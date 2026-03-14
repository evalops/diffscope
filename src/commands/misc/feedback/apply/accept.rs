use crate::core;
use crate::review;

pub(in super::super) fn apply_feedback_accept(
    store: &mut review::FeedbackStore,
    comments: &[core::Comment],
) -> usize {
    let mut updated = 0;
    for comment in comments {
        if review::apply_comment_feedback_signal(store, comment, true) {
            updated += 1;
        }
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
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
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
