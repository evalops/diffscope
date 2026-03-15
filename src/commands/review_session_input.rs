use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::server::state::ReviewSession;

pub(crate) async fn load_review_sessions_input(path: &Path) -> Result<Vec<ReviewSession>> {
    let content = tokio::fs::read_to_string(path).await?;
    load_review_sessions_input_from_str(&content)
}

pub(crate) fn load_review_sessions_input_from_str(content: &str) -> Result<Vec<ReviewSession>> {
    let value = serde_json::from_str(content).map_err(|_| {
        anyhow::anyhow!("Unsupported reviews input format: expected reviews.json map or list")
    })?;
    load_review_sessions_input_from_value(value)
}

pub(crate) fn load_review_sessions_input_from_value(value: Value) -> Result<Vec<ReviewSession>> {
    match value {
        Value::Object(map) if map.values().all(is_review_session_like) => {
            let review_map: HashMap<String, ReviewSession> =
                serde_json::from_value(Value::Object(map))?;
            Ok(review_map
                .into_iter()
                .map(|(review_id, mut session)| {
                    if session.id.trim().is_empty() {
                        session.id = review_id;
                    }
                    session
                })
                .collect())
        }
        Value::Array(items) if items.iter().all(is_review_session_like) => {
            serde_json::from_value(Value::Array(items)).map_err(Into::into)
        }
        _ => anyhow::bail!("Unsupported reviews input format: expected reviews.json map or list"),
    }
}

fn is_review_session_like(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("comments") || object.contains_key("event") || object.contains_key("id")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core;
    use crate::core::comment::{Category, FixEffort, Severity};
    use crate::server::state::{ReviewSession, ReviewStatus};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn sample_comment() -> core::Comment {
        core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "Add a guard for empty inputs".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.81,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    fn sample_session(id: &str) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#7".to_string(),
            github_head_sha: Some("abc123".to_string()),
            github_post_results_requested: None,
            started_at: 1,
            completed_at: Some(2),
            comments: vec![sample_comment()],
            summary: None,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn load_review_sessions_input_supports_review_maps_and_fills_missing_ids() {
        let mut session = sample_session("");
        session.id.clear();
        let json = serde_json::to_string(&HashMap::from([("review-map-id".to_string(), session)]))
            .unwrap();

        let loaded = load_review_sessions_input_from_str(&json).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "review-map-id");
    }

    #[test]
    fn load_review_sessions_input_rejects_non_review_json() {
        let json = serde_json::json!([{"file_path": "src/lib.rs", "content": "oops"}]);

        let error = load_review_sessions_input_from_value(json).unwrap_err();

        assert!(error
            .to_string()
            .contains("expected reviews.json map or list"));
    }
}
