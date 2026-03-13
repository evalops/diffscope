use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticChunk {
    pub key: String,
    pub file_path: PathBuf,
    pub symbol_name: String,
    pub line_range: (usize, usize),
    pub summary: String,
    pub embedding_text: String,
    pub code_excerpt: String,
    pub embedding: Vec<f32>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticIndex {
    pub version: u32,
    pub entries: HashMap<String, SemanticChunk>,
    #[serde(default)]
    pub file_states: HashMap<PathBuf, SemanticFileState>,
    #[serde(default)]
    pub embedding: SemanticEmbeddingMetadata,
}

#[derive(Debug, Clone)]
pub struct SemanticMatch {
    pub chunk: SemanticChunk,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFeedbackExample {
    pub content: String,
    pub category: String,
    pub file_patterns: Vec<String>,
    pub accepted: bool,
    pub created_at: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFeedbackStore {
    pub version: u32,
    pub examples: Vec<SemanticFeedbackExample>,
    #[serde(default)]
    pub embedding: SemanticEmbeddingMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SemanticFileState {
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticEmbeddingMetadata {
    pub strategy: String,
    pub model: String,
    pub dimensions: usize,
}

impl Default for SemanticEmbeddingMetadata {
    fn default() -> Self {
        default_embedding_metadata()
    }
}

impl Default for SemanticIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
            file_states: HashMap::new(),
            embedding: default_embedding_metadata(),
        }
    }
}

impl Default for SemanticFeedbackStore {
    fn default() -> Self {
        Self {
            version: 1,
            examples: Vec::new(),
            embedding: default_embedding_metadata(),
        }
    }
}

pub(super) fn default_embedding_metadata() -> SemanticEmbeddingMetadata {
    SemanticEmbeddingMetadata {
        strategy: "hash-v1".to_string(),
        model: "local-hash".to_string(),
        dimensions: super::FALLBACK_EMBEDDING_DIMENSIONS,
    }
}

impl SemanticIndex {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(content: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(content)
    }
}

impl SemanticFeedbackStore {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(content: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(content)
    }
}
