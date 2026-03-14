use sha2::{Digest, Sha256};

use crate::adapters::llm::LLMAdapter;

use super::types::default_embedding_metadata;
use super::{SemanticEmbeddingMetadata, SemanticFeedbackStore};

pub fn align_semantic_feedback_store(
    store: &mut SemanticFeedbackStore,
    embedding_adapter: Option<&dyn LLMAdapter>,
) {
    let expected = embedding_metadata_for_adapter(embedding_adapter);
    if !embedding_metadata_compatible(&store.embedding, &expected) {
        store.examples.clear();
    }
    store.embedding = merge_embedding_metadata(&store.embedding, &expected);
}

pub async fn embed_texts_with_fallback(
    adapter: Option<&dyn LLMAdapter>,
    texts: &[String],
) -> Vec<Vec<f32>> {
    if texts.is_empty() {
        return Vec::new();
    }

    if let Some(adapter) = adapter {
        if adapter.supports_embeddings() {
            if let Ok(vectors) = adapter.embed(texts).await {
                if vectors.len() == texts.len() && vectors.iter().all(|vector| !vector.is_empty()) {
                    return vectors;
                }
            }
        }
    }

    texts
        .iter()
        .map(|text| local_hash_embedding(text))
        .collect()
}

pub fn build_feedback_embedding_text(content: &str, category: &str) -> String {
    format!("Category: {category}\nComment: {content}")
}

pub(super) fn local_hash_embedding(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; super::FALLBACK_EMBEDDING_DIMENSIONS];
    let mut seen = 0usize;

    for token in tokenize(text) {
        let hash = Sha256::digest(token.as_bytes());
        let idx =
            ((hash[0] as usize) << 8 | hash[1] as usize) % super::FALLBACK_EMBEDDING_DIMENSIONS;
        let weight = 1.0 + (hash[2] as f32 / 255.0);
        if hash[3] % 2 == 0 {
            vector[idx] += weight;
        } else {
            vector[idx] -= weight;
        }
        seen += 1;
    }

    if seen == 0 {
        return vector;
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }

    vector
}

pub(super) fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for idx in 0..left.len() {
        dot += left[idx] * right[idx];
        left_norm += left[idx] * left[idx];
        right_norm += right[idx] * right[idx];
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0)
}

pub(super) fn embedding_metadata_for_adapter(
    adapter: Option<&dyn LLMAdapter>,
) -> SemanticEmbeddingMetadata {
    match adapter {
        Some(adapter) if adapter.supports_embeddings() => SemanticEmbeddingMetadata {
            strategy: "native".to_string(),
            model: adapter.model_name().to_string(),
            dimensions: 0,
        },
        _ => default_embedding_metadata(),
    }
}

pub(super) fn embedding_metadata_compatible(
    existing: &SemanticEmbeddingMetadata,
    expected: &SemanticEmbeddingMetadata,
) -> bool {
    existing.strategy == expected.strategy
        && existing.model == expected.model
        && (existing.dimensions == 0
            || expected.dimensions == 0
            || existing.dimensions == expected.dimensions)
}

pub(super) fn merge_embedding_metadata(
    existing: &SemanticEmbeddingMetadata,
    expected: &SemanticEmbeddingMetadata,
) -> SemanticEmbeddingMetadata {
    if !embedding_metadata_compatible(existing, expected) {
        return expected.clone();
    }

    SemanticEmbeddingMetadata {
        strategy: expected.strategy.clone(),
        model: expected.model.clone(),
        dimensions: if expected.dimensions > 0 {
            expected.dimensions
        } else {
            existing.dimensions
        },
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}
