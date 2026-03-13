use std::collections::HashSet;

use crate::review;

use super::super::super::super::{
    FeedbackEvalBucket, FeedbackEvalComment, LoadedFeedbackEvalInput,
};
use super::super::stats::build_bucket;

pub(super) struct FeedbackOverview {
    pub(super) accepted: usize,
    pub(super) rejected: usize,
    pub(super) labeled_reviews: usize,
    pub(super) vague_bucket: FeedbackEvalBucket,
}

pub(super) fn build_feedback_overview(loaded: &LoadedFeedbackEvalInput) -> FeedbackOverview {
    let accepted = loaded
        .comments
        .iter()
        .filter(|comment| comment.accepted)
        .count();
    let rejected = loaded.comments.len().saturating_sub(accepted);
    let labeled_reviews = loaded
        .comments
        .iter()
        .filter_map(|comment| comment.review_id.as_ref())
        .collect::<HashSet<_>>()
        .len();

    let vague_comments: Vec<&FeedbackEvalComment> = loaded
        .comments
        .iter()
        .filter(|comment| review::is_vague_comment_text(&comment.content))
        .collect();
    let vague_accepted = vague_comments
        .iter()
        .filter(|comment| comment.accepted)
        .count();

    FeedbackOverview {
        accepted,
        rejected,
        labeled_reviews,
        vague_bucket: build_bucket("vague".to_string(), vague_comments.len(), vague_accepted),
    }
}
