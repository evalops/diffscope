use std::path::PathBuf;
use tracing::warn;

use crate::config;
use crate::core;

pub(in super::super) fn resolve_convention_store_path(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|dir| dir.join("diffscope").join("conventions.json"))
}

pub(in super::super) fn save_convention_store(
    store: &core::convention_learner::ConventionStore,
    path: &PathBuf,
) {
    if let Ok(json) = store.to_json() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(path, json) {
            warn!(
                "Failed to save convention store to {}: {}",
                path.display(),
                error
            );
        }
    }
}
