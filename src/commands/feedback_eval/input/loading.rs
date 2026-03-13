use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::core;
use crate::server::state::ReviewSession;

use super::super::{FeedbackEvalComment, LoadedFeedbackEvalInput};
use super::conversion::{extend_from_review_session, feedback_comment_from_comment};

pub(in super::super) async fn load_feedback_eval_input(
    path: &Path,
) -> Result<LoadedFeedbackEvalInput> {
    let content = tokio::fs::read_to_string(path).await?;
    load_feedback_eval_input_from_str(&content)
}

pub(in super::super) fn load_feedback_eval_input_from_str(
    content: &str,
) -> Result<LoadedFeedbackEvalInput> {
    if let Ok(review_map) = serde_json::from_str::<HashMap<String, ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for (review_id, session) in review_map {
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(review_list) = serde_json::from_str::<Vec<ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for session in review_list {
            let review_id = session.id.clone();
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(store) = serde_json::from_str::<core::SemanticFeedbackStore>(content) {
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
                category: example.category,
                severity: None,
                confidence: None,
                accepted: example.accepted,
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    if let Ok(comments) = serde_json::from_str::<Vec<core::Comment>>(content) {
        let total_comments_seen = comments.len();
        let comments = comments
            .into_iter()
            .filter_map(|comment| {
                feedback_comment_from_comment("comments-json", None, None, None, None, comment)
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    anyhow::bail!(
        "Unsupported feedback eval input format: expected reviews.json, a comments array, or semantic feedback store JSON"
    )
}
