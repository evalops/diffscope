use anyhow::Result;

use crate::adapters;
use crate::config;
use crate::core;

use super::patterns::derive_file_patterns;

#[allow(dead_code)]
pub async fn record_semantic_feedback_example(
    config: &config::Config,
    comment: &core::Comment,
    accepted: bool,
) -> Result<()> {
    record_semantic_feedback_examples(config, std::slice::from_ref(comment), accepted).await?;
    Ok(())
}

pub async fn record_semantic_feedback_examples(
    config: &config::Config,
    comments: &[core::Comment],
    accepted: bool,
) -> Result<usize> {
    if comments.is_empty() {
        return Ok(0);
    }

    let semantic_path = core::default_semantic_feedback_path(&config.feedback_path);
    let mut store = core::load_semantic_feedback_store(&semantic_path);
    let model_config = config.to_model_config_for_role(config::ModelRole::Embedding);
    let adapter = adapters::llm::create_adapter(&model_config).ok();
    core::align_semantic_feedback_store(&mut store, adapter.as_deref());

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(adapter.as_deref(), &embedding_texts).await;
    let before = store.examples.len();
    let timestamp = chrono::Utc::now().to_rfc3339();

    for (comment, embedding) in comments.iter().zip(embeddings.into_iter()) {
        store.add_example(core::SemanticFeedbackExample {
            content: comment.content.clone(),
            category: comment.category.as_str().to_string(),
            file_patterns: derive_file_patterns(&comment.file_path),
            accepted,
            created_at: timestamp.clone(),
            embedding,
        });
    }

    core::save_semantic_feedback_store(&semantic_path, &store)?;
    Ok(store.examples.len().saturating_sub(before))
}
