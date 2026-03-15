use anyhow::Result;
use serde_json::Value;

use crate::core;
use crate::server::state::ReviewSession;

use super::super::super::{FeedbackEvalComment, LoadedFeedbackEvalInput};
use super::super::conversion::{extend_from_review_session, feedback_comment_from_comment};
use super::format::FeedbackEvalInputFormat;

pub(super) fn load_feedback_eval_input_from_value(
    value: Value,
    input_format: FeedbackEvalInputFormat,
) -> Result<LoadedFeedbackEvalInput> {
    match input_format {
        FeedbackEvalInputFormat::ReviewMap | FeedbackEvalInputFormat::ReviewList => {
            load_feedback_eval_input_from_review_sessions(
                crate::commands::load_review_sessions_input_from_value(value)?,
            )
        }
        FeedbackEvalInputFormat::SemanticStore => {
            load_feedback_eval_input_from_semantic_store(serde_json::from_value(value)?)
        }
        FeedbackEvalInputFormat::CommentsJson => {
            load_feedback_eval_input_from_comments_json(serde_json::from_value(value)?)
        }
    }
}

fn load_feedback_eval_input_from_review_sessions(
    review_sessions: Vec<ReviewSession>,
) -> Result<LoadedFeedbackEvalInput> {
    let mut loaded = LoadedFeedbackEvalInput::default();
    for session in review_sessions {
        let review_id = session.id.clone();
        extend_from_review_session(&mut loaded, Some(review_id), session);
    }
    Ok(loaded)
}

fn load_feedback_eval_input_from_semantic_store(
    store: core::SemanticFeedbackStore,
) -> Result<LoadedFeedbackEvalInput> {
    let total_comments_seen = store.examples.len();
    let comments = store
        .examples
        .into_iter()
        .map(|example| FeedbackEvalComment {
            source_kind: "semantic-feedback".to_string(),
            review_id: None,
            repo: None,
            pr_number: None,
            title: None,
            file_path: None,
            line_number: None,
            file_patterns: example.file_patterns,
            content: example.content,
            rule_id: None,
            category: example.category,
            severity: None,
            confidence: None,
            accepted: example.accepted,
        })
        .collect();

    Ok(LoadedFeedbackEvalInput {
        total_comments_seen,
        total_reviews_seen: 0,
        comments,
    })
}

fn load_feedback_eval_input_from_comments_json(
    comments: Vec<core::Comment>,
) -> Result<LoadedFeedbackEvalInput> {
    let total_comments_seen = comments.len();
    let comments = comments
        .into_iter()
        .filter_map(|comment| {
            feedback_comment_from_comment("comments-json", None, None, None, None, comment)
        })
        .collect();

    Ok(LoadedFeedbackEvalInput {
        total_comments_seen,
        total_reviews_seen: 0,
        comments,
    })
}
