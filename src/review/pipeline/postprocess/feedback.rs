#[path = "feedback/annotate.rs"]
mod annotate;
#[path = "feedback/lookup.rs"]
mod lookup;

use crate::adapters;
use crate::config;
use crate::core;

use super::super::comments::is_analyzer_comment;
use annotate::adjust_comment_from_feedback;
use lookup::lookup_semantic_feedback_observations;

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

            let observations =
                lookup_semantic_feedback_observations(store, &embedding, &comment, config);
            adjust_comment_from_feedback(&mut comment, observations, config);
            comment
        })
        .collect()
}
