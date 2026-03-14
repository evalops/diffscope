#[path = "input/conversion.rs"]
mod conversion;
#[path = "input/loading.rs"]
mod loading;

pub(super) use loading::load_feedback_eval_input;

#[cfg(test)]
use loading::load_feedback_eval_input_from_str;

#[cfg(test)]
mod tests {
    use super::load_feedback_eval_input_from_str;
    use crate::core;
    use crate::core::comment::{Category, FixEffort, ReviewSummary, Severity};
    use crate::server::state::{ReviewSession, ReviewStatus};
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
            id: format!("{content}-{confidence}"),
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
