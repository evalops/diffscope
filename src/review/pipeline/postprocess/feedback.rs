use crate::adapters;
use crate::config;
use crate::core;

use super::super::super::feedback::derive_file_patterns;
use super::super::comments::is_analyzer_comment;

pub(super) async fn apply_semantic_feedback_adjustment(
    comments: Vec<core::Comment>,
    store: Option<&core::SemanticFeedbackStore>,
    embedding_adapter: Option<&dyn adapters::llm::LLMAdapter>,
    config: &config::Config,
) -> Vec<core::Comment> {
    let Some(store) = store else {
        return comments;
    };
    if store.examples.len() < config.semantic_feedback_min_examples {
        return comments;
    }

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(embedding_adapter, &embedding_texts).await;

    comments
        .into_iter()
        .zip(embeddings)
        .map(|(mut comment, embedding)| {
            if is_analyzer_comment(&comment) {
                return comment;
            }

            let file_patterns = derive_file_patterns(&comment.file_path);
            let matches = core::find_similar_feedback_examples(
                store,
                &embedding,
                comment.category.as_str(),
                &file_patterns,
                config.semantic_feedback_similarity,
                config.semantic_feedback_max_neighbors,
            );
            let accepted = matches
                .iter()
                .filter(|(example, _)| example.accepted)
                .count();
            let rejected = matches
                .iter()
                .filter(|(example, _)| !example.accepted)
                .count();
            let observations = accepted + rejected;

            if observations < config.semantic_feedback_min_examples {
                return comment;
            }

            if rejected > accepted {
                let delta = ((rejected - accepted) as f32 * 0.15).min(0.45);
                comment.confidence = (comment.confidence - delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:rejected")
                {
                    comment.tags.push("semantic-feedback:rejected".to_string());
                }
            } else if accepted > rejected {
                let delta = ((accepted - rejected) as f32 * 0.10).min(0.25);
                comment.confidence = (comment.confidence + delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:accepted")
                {
                    comment.tags.push("semantic-feedback:accepted".to_string());
                }
            }

            comment
        })
        .collect()
}
