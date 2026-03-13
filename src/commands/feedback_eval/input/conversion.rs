use crate::core;
use crate::review;
use crate::server::state::ReviewSession;

use super::super::{FeedbackEvalComment, LoadedFeedbackEvalInput};

pub(super) fn extend_from_review_session(
    loaded: &mut LoadedFeedbackEvalInput,
    review_id: Option<String>,
    session: ReviewSession,
) {
    let repo = session
        .event
        .as_ref()
        .and_then(|event| event.github_repo.clone());
    let pr_number = session.event.as_ref().and_then(|event| event.github_pr);
    let title = session.event.as_ref().and_then(|event| event.title.clone());

    loaded.total_reviews_seen += 1;
    loaded.total_comments_seen += session.comments.len();
    loaded
        .comments
        .extend(session.comments.into_iter().filter_map(|comment| {
            feedback_comment_from_comment(
                "review-session",
                review_id.clone(),
                repo.clone(),
                pr_number,
                title.clone(),
                comment,
            )
        }));
}

pub(super) fn feedback_comment_from_comment(
    source_kind: &str,
    review_id: Option<String>,
    repo: Option<String>,
    pr_number: Option<u32>,
    title: Option<String>,
    comment: core::Comment,
) -> Option<FeedbackEvalComment> {
    let accepted = normalize_feedback_label(comment.feedback.as_deref()?)?;
    let file_patterns = review::derive_file_patterns(&comment.file_path);

    Some(FeedbackEvalComment {
        source_kind: source_kind.to_string(),
        review_id,
        repo,
        pr_number,
        title,
        file_path: Some(comment.file_path),
        line_number: Some(comment.line_number),
        file_patterns,
        content: comment.content,
        category: comment.category.to_string(),
        severity: Some(comment.severity.to_string()),
        confidence: Some(comment.confidence),
        accepted,
    })
}

fn normalize_feedback_label(label: &str) -> Option<bool> {
    match label.trim().to_ascii_lowercase().as_str() {
        "accept" | "accepted" => Some(true),
        "reject" | "rejected" => Some(false),
        _ => None,
    }
}
