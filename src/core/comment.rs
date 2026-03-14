use anyhow::Result;
#[path = "comment/classify.rs"]
mod classify;
#[path = "comment/confidence.rs"]
mod confidence;
#[path = "comment/identity.rs"]
mod identity;
#[path = "comment/ordering.rs"]
mod ordering;
#[path = "comment/signals.rs"]
mod signals;
#[path = "comment/suggestions.rs"]
mod suggestions;
#[path = "comment/summary.rs"]
mod summary;
#[path = "comment/tags.rs"]
mod tags;
#[path = "comment/types.rs"]
mod types;

use classify::{determine_category, determine_fix_effort, determine_severity};
use confidence::calculate_confidence;
use ordering::{
    deduplicate_comments as deduplicate_comment_list,
    sort_by_priority as order_comments_by_priority,
};
#[cfg(test)]
use std::path::PathBuf;
use suggestions::generate_code_suggestion;
use summary::{
    apply_review_runtime_state as apply_review_runtime_summary_state,
    apply_verification as apply_verification_summary, generate_summary as build_review_summary,
    inherit_review_state as inherit_review_summary_state,
};
use tags::extract_tags;

pub use identity::compute_comment_id;
pub use types::{
    Category, CodeSuggestion, Comment, CommentStatus, FixEffort, MergeReadiness, RawComment,
    ReviewSummary, ReviewVerificationState, ReviewVerificationSummary, Severity,
};

pub struct CommentSynthesizer;

impl CommentSynthesizer {
    pub fn synthesize(raw_comments: Vec<RawComment>) -> Result<Vec<Comment>> {
        let mut comments = Vec::new();

        for raw in raw_comments {
            comments.push(Self::process_raw_comment(raw)?);
        }

        deduplicate_comment_list(&mut comments);
        sort_comments_by_priority(&mut comments);

        Ok(comments)
    }

    pub fn generate_summary(comments: &[Comment]) -> ReviewSummary {
        build_review_summary(comments)
    }

    pub fn inherit_review_state(
        summary: ReviewSummary,
        previous: Option<&ReviewSummary>,
    ) -> ReviewSummary {
        inherit_review_summary_state(summary, previous)
    }

    pub fn apply_verification(
        summary: ReviewSummary,
        verification: ReviewVerificationSummary,
    ) -> ReviewSummary {
        apply_verification_summary(summary, verification)
    }

    pub fn apply_runtime_review_state(summary: ReviewSummary, stale_review: bool) -> ReviewSummary {
        apply_review_runtime_summary_state(summary, stale_review)
    }

    fn process_raw_comment(raw: RawComment) -> Result<Comment> {
        let lower = raw.content.to_lowercase();
        let severity = raw
            .severity
            .clone()
            .unwrap_or_else(|| determine_severity(&lower));
        let category = raw
            .category
            .clone()
            .unwrap_or_else(|| determine_category(&lower));
        let confidence = raw
            .confidence
            .unwrap_or_else(|| calculate_confidence(&lower, &severity, &category));
        let confidence = confidence.clamp(0.0, 1.0);
        let tags = if raw.tags.is_empty() {
            extract_tags(&lower, &category)
        } else {
            raw.tags.clone()
        };
        let fix_effort = raw
            .fix_effort
            .clone()
            .unwrap_or_else(|| determine_fix_effort(&lower, &category));
        let code_suggestion = generate_code_suggestion(&raw);
        let id = compute_comment_id(&raw.file_path, &raw.content, &category);

        Ok(Comment {
            id,
            file_path: raw.file_path,
            line_number: raw.line_number,
            content: raw.content,
            rule_id: raw.rule_id,
            severity,
            category,
            suggestion: raw.suggestion,
            confidence,
            code_suggestion,
            tags,
            fix_effort,
            feedback: None,
            status: CommentStatus::Open,
        })
    }
}

pub fn sort_comments_by_priority(comments: &mut [Comment]) {
    order_comments_by_priority(comments);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw_comment(content: &str) -> RawComment {
        RawComment {
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: content.to_string(),
            rule_id: None,
            suggestion: None,
            severity: None,
            category: None,
            confidence: None,
            fix_effort: None,
            tags: Vec::new(),
            code_suggestion: None,
        }
    }

    fn synthesize_single(content: &str) -> Comment {
        CommentSynthesizer::process_raw_comment(make_raw_comment(content)).unwrap()
    }

    #[test]
    fn test_deduplicate_preserves_highest_severity() {
        // Regression: dedup_by keeps the first element of a consecutive pair,
        // but doesn't consider severity. If Warning comes before Error
        // (due to stable sort on file/line/content), the Error is dropped.
        let raw_comments = vec![
            RawComment {
                file_path: PathBuf::from("src/lib.rs"),
                line_number: 10,
                content: "Missing null check".to_string(),
                rule_id: None,
                suggestion: None,
                severity: Some(Severity::Warning),
                category: Some(Category::Bug),
                confidence: Some(0.8),
                fix_effort: None,
                tags: Vec::new(),
                code_suggestion: None,
            },
            RawComment {
                file_path: PathBuf::from("src/lib.rs"),
                line_number: 10,
                content: "Missing null check".to_string(),
                rule_id: None,
                suggestion: None,
                severity: Some(Severity::Error),
                category: Some(Category::Bug),
                confidence: Some(0.9),
                fix_effort: None,
                tags: Vec::new(),
                code_suggestion: None,
            },
        ];

        let comments = CommentSynthesizer::synthesize(raw_comments).unwrap();
        assert_eq!(comments.len(), 1, "Should deduplicate to one comment");
        assert_eq!(
            comments[0].severity,
            Severity::Error,
            "Should keep the higher severity (Error), not the lower (Warning)"
        );
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Error.to_string(), "Error");
        assert_eq!(Severity::Warning.to_string(), "Warning");
        assert_eq!(Severity::Info.to_string(), "Info");
        assert_eq!(Severity::Suggestion.to_string(), "Suggestion");
    }

    #[test]
    fn test_category_display() {
        assert_eq!(Category::Bug.to_string(), "Bug");
        assert_eq!(Category::Security.to_string(), "Security");
        assert_eq!(Category::Performance.to_string(), "Performance");
        assert_eq!(Category::Style.to_string(), "Style");
        assert_eq!(Category::Documentation.to_string(), "Documentation");
        assert_eq!(Category::BestPractice.to_string(), "BestPractice");
        assert_eq!(Category::Maintainability.to_string(), "Maintainability");
        assert_eq!(Category::Testing.to_string(), "Testing");
        assert_eq!(Category::Architecture.to_string(), "Architecture");
    }

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Info.as_str(), "info");
        assert_eq!(Severity::Suggestion.as_str(), "suggestion");
    }

    #[test]
    fn test_category_as_str() {
        assert_eq!(Category::Bug.as_str(), "bug");
        assert_eq!(Category::Security.as_str(), "security");
        assert_eq!(Category::BestPractice.as_str(), "bestpractice");
    }

    #[test]
    fn test_security_regression_cases_are_classified_and_tagged() {
        let cases = [
            (
                "Running as root in Docker container (CWE-250)",
                Category::Security,
                vec!["docker", "root-container", "cwe-250"],
            ),
            (
                "Unsafe deserialization via pickle.load can trigger RCE (CWE-502)",
                Category::Security,
                vec!["deserialization", "cwe-502"],
            ),
            (
                "JWT verification is missing and enables auth bypass (CWE-347)",
                Category::Security,
                vec!["jwt", "cwe-347"],
            ),
        ];

        for (content, expected_category, expected_tags) in cases {
            let comment = synthesize_single(content);
            assert_eq!(comment.category, expected_category, "content: {content}");
            for tag in expected_tags {
                assert!(
                    comment.tags.iter().any(|existing| existing == tag),
                    "missing tag `{tag}` for content `{content}`: {:?}",
                    comment.tags
                );
            }
        }
    }

    #[test]
    fn test_extract_tags_collects_multiple_cwes() {
        let comment = synthesize_single(
            "SQL injection (CWE-89) can combine with XSS (CWE-79) in the same flow",
        );
        assert!(comment.tags.iter().any(|tag| tag == "cwe-89"));
        assert!(comment.tags.iter().any(|tag| tag == "cwe-79"));
    }

    #[test]
    fn test_deserialization_does_not_trigger_weak_cipher_tag() {
        let comment = synthesize_single("Unsafe deserialization via yaml.load on untrusted input");
        assert!(comment.tags.iter().any(|tag| tag == "deserialization"));
        assert!(!comment.tags.iter().any(|tag| tag == "weak-cipher"));
    }

    #[test]
    fn test_generate_code_suggestion_accepts_more_action_words() {
        let mut raw = make_raw_comment("Missing null check before dereference");
        raw.suggestion = Some("Add a guard clause before dereferencing the value".to_string());

        let comment = CommentSynthesizer::process_raw_comment(raw).unwrap();
        assert!(comment.code_suggestion.is_some());
    }

    #[test]
    fn test_generate_code_suggestion_ignores_non_action_words() {
        let mut raw = make_raw_comment("Suggestion parsing regression");
        raw.suggestion = Some("Reusable helper already exists for this code path".to_string());

        let comment = CommentSynthesizer::process_raw_comment(raw).unwrap();
        assert!(comment.code_suggestion.is_none());
    }
}
