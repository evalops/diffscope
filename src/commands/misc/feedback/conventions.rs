use std::path::PathBuf;

use crate::config;
use crate::core;
use crate::core::convention_learner::ConventionStore;
use crate::review;

pub(super) fn record_convention_feedback(
    config: &config::Config,
    comments: &[core::Comment],
    is_accepted: bool,
) {
    let Some(convention_path) = resolve_convention_store_path_for_feedback(config) else {
        return;
    };

    let json = std::fs::read_to_string(&convention_path).ok();
    let mut store = json
        .as_deref()
        .and_then(|value| ConventionStore::from_json(value).ok())
        .unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();
    for comment in comments {
        let file_patterns = review::derive_file_patterns(&comment.file_path);
        store.record_feedback(
            &comment.content,
            &comment.category.to_string(),
            is_accepted,
            file_patterns.first().map(String::as_str),
            &now,
        );
    }

    if let Ok(out_json) = store.to_json() {
        if let Some(parent) = convention_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(convention_path, out_json);
    }
}

fn resolve_convention_store_path_for_feedback(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|dir| dir.join("diffscope").join("conventions.json"))
}
