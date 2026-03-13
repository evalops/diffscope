use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::core;
use crate::review;
use crate::server::state::ReviewSession;

use super::{FeedbackEvalComment, LoadedFeedbackEvalInput};

pub(super) async fn load_feedback_eval_input(path: &Path) -> Result<LoadedFeedbackEvalInput> {
    let content = tokio::fs::read_to_string(path).await?;
    load_feedback_eval_input_from_str(&content)
}

pub(super) fn load_feedback_eval_input_from_str(content: &str) -> Result<LoadedFeedbackEvalInput> {
    if let Ok(review_map) = serde_json::from_str::<HashMap<String, ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for (review_id, session) in review_map {
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(review_list) = serde_json::from_str::<Vec<ReviewSession>>(content) {
        let mut loaded = LoadedFeedbackEvalInput::default();
        for session in review_list {
            let review_id = session.id.clone();
            extend_from_review_session(&mut loaded, Some(review_id), session);
        }
        return Ok(loaded);
    }

    if let Ok(store) = serde_json::from_str::<core::SemanticFeedbackStore>(content) {
        let total_comments_seen = store.examples.len();
        let comments = store
            .examples
            .into_iter()
            .map(|example| FeedbackEvalComment {
                source_kind: "semantic-feedback".to_string(),
                review_id: None,
                repo: None,
                pr_number: None,
                title: None,
                file_path: None,
                line_number: None,
                file_patterns: example.file_patterns,
                content: example.content,
                category: example.category,
                severity: None,
                confidence: None,
                accepted: example.accepted,
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    if let Ok(comments) = serde_json::from_str::<Vec<core::Comment>>(content) {
        let total_comments_seen = comments.len();
        let comments = comments
            .into_iter()
            .filter_map(|comment| {
                feedback_comment_from_comment("comments-json", None, None, None, None, comment)
            })
            .collect();
        return Ok(LoadedFeedbackEvalInput {
            total_comments_seen,
            total_reviews_seen: 0,
            comments,
        });
    }

    anyhow::bail!(
        "Unsupported feedback eval input format: expected reviews.json, a comments array, or semantic feedback store JSON"
    )
}

fn extend_from_review_session(
    loaded: &mut LoadedFeedbackEvalInput,
    review_id: Option<String>,
    session: ReviewSession,
) {
    let repo = session
        .event
        .as_ref()
        .and_then(|event| event.github_repo.clone());
    let pr_number = session.event.as_ref().and_then(|event| event.github_pr);
    let title = session.event.as_ref().and_then(|event| event.title.clone());

    loaded.total_reviews_seen += 1;
    loaded.total_comments_seen += session.comments.len();
    loaded
        .comments
        .extend(session.comments.into_iter().filter_map(|comment| {
            feedback_comment_from_comment(
                "review-session",
                review_id.clone(),
                repo.clone(),
                pr_number,
                title.clone(),
                comment,
            )
        }));
}

fn feedback_comment_from_comment(
    source_kind: &str,
    review_id: Option<String>,
    repo: Option<String>,
    pr_number: Option<u32>,
    title: Option<String>,
    comment: core::Comment,
) -> Option<FeedbackEvalComment> {
    let accepted = normalize_feedback_label(comment.feedback.as_deref()?)?;
    let file_patterns = review::derive_file_patterns(&comment.file_path);

    Some(FeedbackEvalComment {
        source_kind: source_kind.to_string(),
        review_id,
        repo,
        pr_number,
        title,
        file_path: Some(comment.file_path),
        line_number: Some(comment.line_number),
        file_patterns,
        content: comment.content,
        category: comment.category.to_string(),
        severity: Some(comment.severity.to_string()),
        confidence: Some(comment.confidence),
        accepted,
    })
}

fn normalize_feedback_label(label: &str) -> Option<bool> {
    match label.trim().to_ascii_lowercase().as_str() {
        "accept" | "accepted" => Some(true),
        "reject" | "rejected" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, ReviewSummary, Severity};
    use crate::server::state::ReviewStatus;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_comment(
        content: &str,
        feedback: Option<&str>,
        confidence: f32,
        category: Category,
        severity: Severity,
    ) -> core::Comment {
        core::Comment {
            id: format!("{}-{}", content, confidence),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: content.to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: feedback.map(str::to_string),
        }
    }

    fn make_review_session(comments: Vec<core::Comment>) -> ReviewSession {
        ReviewSession {
            id: "review-1".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "raw".to_string(),
            started_at: 1,
            completed_at: Some(2),
            comments,
            summary: None::<ReviewSummary>,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn load_feedback_eval_input_supports_review_session_maps() {
        let session = make_review_session(vec![
            make_comment(
                "Concrete issue",
                Some("accept"),
                0.9,
                Category::Security,
                Severity::Warning,
            ),
            make_comment("Unlabeled issue", None, 0.4, Category::Bug, Severity::Info),
        ]);
        let json =
            serde_json::to_string(&HashMap::from([("review-1".to_string(), session)])).unwrap();

        let loaded = load_feedback_eval_input_from_str(&json).unwrap();

        assert_eq!(loaded.total_reviews_seen, 1);
        assert_eq!(loaded.total_comments_seen, 2);
        assert_eq!(loaded.comments.len(), 1);
        assert_eq!(loaded.comments[0].review_id.as_deref(), Some("review-1"));
        assert!(loaded.comments[0].accepted);
    }

    #[test]
    fn load_feedback_eval_input_supports_semantic_feedback_store() {
        let json = serde_json::to_string(&core::SemanticFeedbackStore {
            version: 1,
            examples: vec![core::SemanticFeedbackExample {
                content: "Consider adding a null check".to_string(),
                category: "Bug".to_string(),
                file_patterns: vec!["*.rs".to_string()],
                accepted: false,
                created_at: "2026-03-13T00:00:00Z".to_string(),
                embedding: vec![],
            }],
            embedding: Default::default(),
        })
        .unwrap();

        let loaded = load_feedback_eval_input_from_str(&json).unwrap();

        assert_eq!(loaded.total_comments_seen, 1);
        assert_eq!(loaded.comments.len(), 1);
        assert_eq!(loaded.comments[0].source_kind, "semantic-feedback");
        assert!(!loaded.comments[0].accepted);
    }
}
