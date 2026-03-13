use crate::config;
use crate::core;

use super::super::super::super::feedback::derive_file_patterns;

pub(super) struct SemanticFeedbackObservations {
    pub accepted: usize,
    pub rejected: usize,
}

impl SemanticFeedbackObservations {
    pub(super) fn total(&self) -> usize {
        self.accepted + self.rejected
    }
}

pub(super) fn lookup_semantic_feedback_observations(
    store: &core::SemanticFeedbackStore,
    embedding: &[f32],
    comment: &core::Comment,
    config: &config::Config,
) -> SemanticFeedbackObservations {
    let file_patterns = derive_file_patterns(&comment.file_path);
    let matches = core::find_similar_feedback_examples(
        store,
        embedding,
        comment.category.as_str(),
        &file_patterns,
        config.semantic_feedback_similarity,
        config.semantic_feedback_max_neighbors,
    );

    SemanticFeedbackObservations {
        accepted: matches
            .iter()
            .filter(|(example, _)| example.accepted)
            .count(),
        rejected: matches
            .iter()
            .filter(|(example, _)| !example.accepted)
            .count(),
    }
}
