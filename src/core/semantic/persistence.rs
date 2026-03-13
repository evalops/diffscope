use anyhow::Result;
use std::path::{Path, PathBuf};

use super::{SemanticFeedbackStore, SemanticIndex};

pub fn default_index_path(repo_root: &Path) -> PathBuf {
    let repo_key = super::hash_text(&repo_root.to_string_lossy());
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("diffscope")
        .join("semantic")
        .join(format!("{}.json", &repo_key[..16]))
}

pub fn default_semantic_feedback_path(feedback_path: &Path) -> PathBuf {
    let parent = feedback_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = feedback_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("diffscope.feedback");
    parent.join(format!("{}.semantic.json", stem))
}

pub fn load_semantic_index(path: &Path) -> SemanticIndex {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| SemanticIndex::from_json(&content).ok())
        .unwrap_or_default()
}

pub fn save_semantic_index(path: &Path, index: &SemanticIndex) -> Result<()> {
    atomic_write_string(path, &index.to_json()?)
}

pub fn load_semantic_feedback_store(path: &Path) -> SemanticFeedbackStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| SemanticFeedbackStore::from_json(&content).ok())
        .unwrap_or_default()
}

pub fn save_semantic_feedback_store(path: &Path, store: &SemanticFeedbackStore) -> Result<()> {
    atomic_write_string(path, &store.to_json()?)
}

fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("semantic.json");
    let tmp_path = path.with_file_name(format!("{}.{}.tmp", file_name, std::process::id()));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
