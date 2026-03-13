use crate::server::state::ReviewSession;

use super::super::super::LoadedFeedbackEvalInput;
use super::comment::feedback_comment_from_comment;

pub(in super::super) fn extend_from_review_session(
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
