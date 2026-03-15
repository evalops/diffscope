use super::{Comment, CommentOutcome, CommentStatus};

pub fn derive_comment_outcomes(comment: &Comment, stale_review: bool) -> Vec<CommentOutcome> {
    let mut outcomes = Vec::new();

    match comment
        .feedback
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("accept") => outcomes.push(CommentOutcome::Accepted),
        Some("reject") => outcomes.push(CommentOutcome::Rejected),
        _ => {}
    }

    if comment.status == CommentStatus::Resolved {
        outcomes.push(CommentOutcome::Addressed);
    }

    if comment.status == CommentStatus::Open && stale_review {
        outcomes.push(CommentOutcome::Stale);
    }

    if outcomes.is_empty() && comment.status == CommentStatus::Open {
        outcomes.push(CommentOutcome::New);
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::core::comment::{Category, Comment, FixEffort, Severity};

    use super::*;

    fn make_comment() -> Comment {
        Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn derives_new_for_open_comments_without_other_signals() {
        assert_eq!(
            derive_comment_outcomes(&make_comment(), false),
            vec![CommentOutcome::New]
        );
    }

    #[test]
    fn derives_feedback_and_addressed_outcomes_independently() {
        let mut comment = make_comment();
        comment.feedback = Some("accept".to_string());
        comment.status = CommentStatus::Resolved;

        assert_eq!(
            derive_comment_outcomes(&comment, false),
            vec![CommentOutcome::Accepted, CommentOutcome::Addressed]
        );
    }

    #[test]
    fn derives_rejected_without_marking_new() {
        let mut comment = make_comment();
        comment.feedback = Some("reject".to_string());

        assert_eq!(
            derive_comment_outcomes(&comment, false),
            vec![CommentOutcome::Rejected]
        );
    }

    #[test]
    fn derives_stale_for_open_comments_in_stale_reviews() {
        assert_eq!(
            derive_comment_outcomes(&make_comment(), true),
            vec![CommentOutcome::Stale]
        );
    }

    #[test]
    fn dismissed_comments_keep_lifecycle_without_derived_outcomes() {
        let mut comment = make_comment();
        comment.status = CommentStatus::Dismissed;

        assert!(derive_comment_outcomes(&comment, false).is_empty());
    }
}
