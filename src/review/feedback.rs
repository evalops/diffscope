use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::config;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackTypeStats {
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackStore {
    #[serde(default)]
    pub suppress: HashSet<String>,
    #[serde(default)]
    pub accept: HashSet<String>,
    #[serde(default)]
    pub by_comment_type: HashMap<String, FeedbackTypeStats>,
}

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
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_store_default_is_empty() {
        let store = FeedbackStore::default();
        assert!(store.suppress.is_empty());
        assert!(store.accept.is_empty());
        assert!(store.by_comment_type.is_empty());
    }

    #[test]
    fn feedback_store_roundtrip_json() {
        let mut store = FeedbackStore::default();
        store.suppress.insert("c1".to_string());
        store.accept.insert("c2".to_string());
        store.by_comment_type.insert(
            "style".to_string(),
            FeedbackTypeStats {
                accepted: 1,
                rejected: 2,
            },
        );

        let json = serde_json::to_string(&store).unwrap();
        let deserialized: FeedbackStore = serde_json::from_str(&json).unwrap();
        assert!(deserialized.suppress.contains("c1"));
        assert!(deserialized.accept.contains("c2"));
        assert_eq!(deserialized.by_comment_type["style"].accepted, 1);
        assert_eq!(deserialized.by_comment_type["style"].rejected, 2);
    }

    #[test]
    fn load_feedback_store_from_nonexistent_path_returns_default() {
        let store = load_feedback_store_from_path(Path::new("/nonexistent/path.json"));
        assert!(store.suppress.is_empty());
    }
}
