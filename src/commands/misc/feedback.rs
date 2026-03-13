use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::core;
use crate::core::convention_learner::ConventionStore;
use crate::review;

pub async fn feedback_command(
    mut config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<()> {
    let (action, input_path) = match (accept, reject) {
        (Some(path), None) => ("accept", path),
        (None, Some(path)) => ("reject", path),
        _ => {
            anyhow::bail!("Specify exactly one of --accept or --reject");
        }
    };

    let feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    config.feedback_path = feedback_path.clone();
    let content = tokio::fs::read_to_string(&input_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let mut store = review::load_feedback_store_from_path(&feedback_path);

    let updated = if action == "accept" {
        apply_feedback_accept(&mut store, &comments)
    } else {
        apply_feedback_reject(&mut store, &comments)
    };

    review::save_feedback_store(&feedback_path, &store)?;
    println!(
        "Updated feedback store at {} ({} {} comment(s))",
        feedback_path.display(),
        updated,
        action
    );

    let is_accepted = action == "accept";
    let _ = review::record_semantic_feedback_examples(&config, &comments, is_accepted).await;

    let convention_path = resolve_convention_store_path_for_feedback(&config);
    if let Some(ref cpath) = convention_path {
        let json = std::fs::read_to_string(cpath).ok();
        let mut cstore = json
            .as_deref()
            .and_then(|j| ConventionStore::from_json(j).ok())
            .unwrap_or_default();
        let now = chrono::Utc::now().to_rfc3339();
        for comment in &comments {
            let file_patterns = review::derive_file_patterns(&comment.file_path);
            cstore.record_feedback(
                &comment.content,
                &comment.category.to_string(),
                is_accepted,
                file_patterns.first().map(String::as_str),
                &now,
            );
        }
        if let Ok(out_json) = cstore.to_json() {
            if let Some(parent) = cpath.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(cpath, out_json);
        }
    }

    Ok(())
}

fn apply_feedback_accept(store: &mut review::FeedbackStore, comments: &[core::Comment]) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.accept.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.accepted = stats.accepted.saturating_add(1);
            let file_patterns = review::derive_file_patterns(&comment.file_path);
            store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, true);
        }
        store.suppress.remove(&comment.id);
    }
    updated
}

fn apply_feedback_reject(store: &mut review::FeedbackStore, comments: &[core::Comment]) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.suppress.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.rejected = stats.rejected.saturating_add(1);
            let file_patterns = review::derive_file_patterns(&comment.file_path);
            store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, false);
        }
        store.accept.remove(&comment.id);
    }
    updated
}

fn resolve_convention_store_path_for_feedback(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|d| d.join("diffscope").join("conventions.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feedback_stats_not_double_counted() {
        let mut store = review::FeedbackStore::default();
        let comment = core::Comment {
            id: "cmt_dup".to_string(),
            file_path: PathBuf::from("test.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        };

        let comments = vec![comment];

        for _ in 0..2 {
            apply_feedback_accept(&mut store, &comments);
        }

        let key = review::classify_comment_type(&comments[0])
            .as_str()
            .to_string();
        let stats = &store.by_comment_type[&key];
        assert_eq!(
            stats.accepted, 1,
            "Stats should only count 1 acceptance, not 2 (double-counting bug)"
        );
        assert_eq!(store.by_category["Bug"].accepted, 1);
        assert_eq!(store.by_file_pattern["*.rs"].accepted, 1);
        assert_eq!(store.by_category_file_pattern["Bug|*.rs"].accepted, 1);
    }
}
