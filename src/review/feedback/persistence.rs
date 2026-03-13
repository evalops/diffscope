use std::path::Path;

use anyhow::Result;

use crate::config;

use super::store::FeedbackStore;

pub fn load_feedback_store_from_path(path: &Path) -> FeedbackStore {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => FeedbackStore::default(),
    }
}

pub fn load_feedback_store(config: &config::Config) -> FeedbackStore {
    load_feedback_store_from_path(&config.feedback_path)
}

pub fn save_feedback_store(path: &Path, store: &FeedbackStore) -> Result<()> {
    atomic_write_string(path, &serde_json::to_string_pretty(store)?)
}

fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("feedback.json");
    let tmp_path = path.with_file_name(format!("{}.{}.tmp", file_name, std::process::id()));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
