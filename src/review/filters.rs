#[path = "filters/comment_types.rs"]
mod comment_types;
#[path = "filters/confidence.rs"]
mod confidence;
#[path = "filters/run.rs"]
mod run;
#[path = "filters/suppression.rs"]
mod suppression;
#[path = "filters/vague.rs"]
mod vague;

#[allow(unused_imports)]
pub use comment_types::{apply_comment_type_filter, classify_comment_type, ReviewCommentType};
#[allow(unused_imports)]
pub use confidence::{apply_confidence_threshold, apply_feedback_confidence_adjustment};
pub use run::apply_review_filters;
#[allow(unused_imports)]
pub use suppression::{
    apply_feedback_suppression_with_thresholds, should_adaptively_suppress_with_thresholds,
};
#[allow(unused_imports)]
pub use vague::{apply_vague_comment_filter, is_vague_comment_text, is_vague_review_comment};

#[cfg(test)]
mod tests {
    use super::super::feedback::FeedbackStore;
    use super::*;
    use crate::{config, core};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn build_comment(
        id: &str,
        category: core::comment::Category,
        severity: core::comment::Severity,
        confidence: f32,
    ) -> core::Comment {
        core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test comment".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn comment_type_filter_keeps_only_enabled_types() {
        let comments = vec![
            build_comment(
                "logic",
                core::comment::Category::Bug,
                core::comment::Severity::Info,
                0.9,
            ),
            build_comment(
                "style",
                core::comment::Category::Style,
                core::comment::Severity::Suggestion,
                0.9,
            ),
        ];

        let enabled = vec!["logic".to_string()];
        let filtered = apply_comment_type_filter(comments, &enabled);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "logic");
    }

    #[test]
    fn comment_type_filter_empty_enabled_keeps_all() {
        let comments = vec![
            build_comment(
                "c1",
                core::comment::Category::Bug,
                core::comment::Severity::Error,
                0.9,
            ),
            build_comment(
                "c2",
                core::comment::Category::Style,
                core::comment::Severity::Info,
                0.8,
            ),
        ];
        let filtered = apply_comment_type_filter(comments, &[]);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn adaptive_feedback_suppresses_low_priority_comment_types() {
        let feedback = FeedbackStore {
            suppress: HashSet::new(),
            accept: HashSet::new(),
            by_comment_type: HashMap::from([(
                "style".to_string(),
                super::super::feedback::FeedbackTypeStats {
                    accepted: 0,
                    rejected: 3,
                    dismissed: 0,
                },
            )]),
            ..Default::default()
        };

        let comments = vec![
            build_comment(
                "style-low",
                core::comment::Category::Style,
                core::comment::Severity::Suggestion,
                0.95,
            ),
            build_comment(
                "style-high",
                core::comment::Category::Style,
                core::comment::Severity::Error,
                0.95,
            ),
        ];

        let filtered = apply_feedback_suppression_with_thresholds(comments, &feedback, 3, 2);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "style-high");
    }

    #[test]
    fn adaptive_suppression_does_not_suppress_errors() {
        let feedback = FeedbackStore {
            suppress: HashSet::new(),
            accept: HashSet::new(),
            by_comment_type: HashMap::from([(
                "logic".to_string(),
                super::super::feedback::FeedbackTypeStats {
                    accepted: 0,
                    rejected: 10,
                    dismissed: 0,
                },
            )]),
            ..Default::default()
        };

        let comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        assert!(!should_adaptively_suppress_with_thresholds(
            &comment, &feedback, 3, 2
        ));
    }

    #[test]
    fn adaptive_suppression_does_not_suppress_warnings() {
        let feedback = FeedbackStore {
            suppress: HashSet::new(),
            accept: HashSet::new(),
            by_comment_type: HashMap::from([(
                "logic".to_string(),
                super::super::feedback::FeedbackTypeStats {
                    accepted: 0,
                    rejected: 10,
                    dismissed: 0,
                },
            )]),
            ..Default::default()
        };

        let comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        assert!(!should_adaptively_suppress_with_thresholds(
            &comment, &feedback, 3, 2
        ));
    }

    #[test]
    fn confidence_threshold_filters_low_confidence() {
        let comments = vec![
            build_comment(
                "high",
                core::comment::Category::Bug,
                core::comment::Severity::Error,
                0.9,
            ),
            build_comment(
                "low",
                core::comment::Category::Bug,
                core::comment::Severity::Info,
                0.3,
            ),
        ];
        let filtered = apply_confidence_threshold(comments, 0.5);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "high");
    }

    #[test]
    fn confidence_threshold_zero_keeps_all() {
        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.1,
        )];
        let filtered = apply_confidence_threshold(comments, 0.0);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn vague_comment_detection_flags_generic_prefixes() {
        assert!(is_vague_comment_text(
            "Consider adding a guard clause here."
        ));
        assert!(is_vague_comment_text(
            "Ensure this path is covered by tests."
        ));
        assert!(is_vague_comment_text(
            "Make sure the dependency stays aligned."
        ));
    }

    #[test]
    fn vague_comment_detection_preserves_concrete_findings() {
        assert!(!is_vague_comment_text(
            "User-controlled SQL is interpolated directly into the query string."
        ));
    }

    #[test]
    fn vague_filter_drops_generic_comments_but_keeps_concrete_ones() {
        let mut vague = build_comment(
            "vague",
            core::comment::Category::Bug,
            core::comment::Severity::Suggestion,
            0.9,
        );
        vague.content = "Consider adding a guard clause for this input.".to_string();

        let mut concrete = build_comment(
            "concrete",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        concrete.content =
            "This branch dereferences a nil pointer when the config lookup fails.".to_string();
        concrete.suggestion = Some("Return early when the lookup misses.".to_string());

        let filtered = apply_vague_comment_filter(vec![vague, concrete]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "concrete");
    }

    #[test]
    fn strictness_applies_minimum_confidence_floor() {
        let config = config::Config::default();
        let feedback = FeedbackStore::default();
        let comments = vec![build_comment(
            "low-confidence",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        )];

        let filtered = apply_review_filters(comments, &config, &feedback);
        assert!(filtered.is_empty());
    }

    #[test]
    fn classify_comment_type_style() {
        let comment = build_comment(
            "c1",
            core::comment::Category::Style,
            core::comment::Severity::Info,
            0.9,
        );
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Style);
    }

    #[test]
    fn classify_comment_type_informational() {
        let comment = build_comment(
            "c1",
            core::comment::Category::Documentation,
            core::comment::Severity::Info,
            0.9,
        );
        assert_eq!(
            classify_comment_type(&comment),
            ReviewCommentType::Informational
        );
    }

    #[test]
    fn classify_comment_type_syntax() {
        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        comment.content = "This has a syntax error".to_string();
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Syntax);
    }

    #[test]
    fn classify_comment_type_logic_default() {
        let comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Logic);
    }

    #[test]
    fn explicit_feedback_suppression() {
        let mut feedback = FeedbackStore::default();
        feedback.suppress.insert("c1".to_string());

        let comments = vec![
            build_comment(
                "c1",
                core::comment::Category::Bug,
                core::comment::Severity::Error,
                0.9,
            ),
            build_comment(
                "c2",
                core::comment::Category::Bug,
                core::comment::Severity::Error,
                0.9,
            ),
        ];

        let filtered = apply_feedback_suppression_with_thresholds(comments, &feedback, 3, 2);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "c2");
    }

    // ── Feedback confidence adjustment tests ──────────────────────────────

    #[test]
    fn feedback_confidence_no_data_unchanged() {
        let feedback = FeedbackStore::default();
        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        assert_eq!(result[0].confidence, 0.8);
        assert!(result[0].tags.is_empty());
    }

    #[test]
    fn feedback_confidence_below_threshold_unchanged() {
        let mut feedback = FeedbackStore::default();
        // Only 3 observations, below min_observations of 5
        feedback.record_feedback("Bug", None, false);
        feedback.record_feedback("Bug", None, false);
        feedback.record_feedback("Bug", None, false);

        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        assert_eq!(result[0].confidence, 0.8);
    }

    #[test]
    fn feedback_confidence_zero_acceptance_demotes() {
        let mut feedback = FeedbackStore::default();
        for _ in 0..10 {
            feedback.record_feedback("Bug", None, false);
        }

        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        // 0% acceptance now demotes confidence to 75% of the original score.
        assert!(
            (result[0].confidence - 0.6).abs() < 0.01,
            "Got: {}",
            result[0].confidence
        );
        assert!(result[0].tags.contains(&"feedback-calibration".to_string()));
        assert!(result[0]
            .tags
            .contains(&"feedback-calibration:demoted".to_string()));
    }

    #[test]
    fn feedback_confidence_full_acceptance_boosts() {
        let mut feedback = FeedbackStore::default();
        for _ in 0..10 {
            feedback.record_feedback("Bug", None, true);
        }

        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        // 100% acceptance boosts confidence by 25%.
        assert!(
            (result[0].confidence - 1.0).abs() < 0.01,
            "Got: {}",
            result[0].confidence
        );
        assert!(result[0].tags.contains(&"feedback-calibration".to_string()));
        assert!(result[0]
            .tags
            .contains(&"feedback-calibration:boosted".to_string()));
    }

    #[test]
    fn feedback_confidence_composite_key_takes_precedence() {
        let mut feedback = FeedbackStore::default();
        // Category-level: 100% acceptance (would give 1.0x)
        for _ in 0..10 {
            feedback.record_feedback("Bug", None, true);
        }
        // Composite key "Bug|*.rs": 0% acceptance (would give 0.5x)
        for _ in 0..10 {
            feedback.record_feedback("Bug", Some("*.rs"), false);
        }

        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        // Composite key should take precedence over broad category stats.
        // Note: the composite stats include the category-only accepts too,
        // so composite has accepted=0, rejected=10. Category has accepted=10+0=10, rejected=0+10=10
        // Actually record_feedback("Bug", Some("*.rs"), false) adds to by_category too
        // So by_category["Bug"] = accepted:10, rejected:10 = 50% acceptance
        // by_category_file_pattern["Bug|*.rs"] = accepted:0, rejected:10 = 0%
        // Composite wins and applies the stronger demotion.
        assert!(
            (result[0].confidence - 0.6).abs() < 0.01,
            "Got: {}",
            result[0].confidence
        );
    }

    #[test]
    fn feedback_confidence_prefers_path_scoped_buckets() {
        let mut feedback = FeedbackStore::default();
        for _ in 0..10 {
            feedback.record_feedback("Bug", None, true);
            feedback.record_feedback_patterns("Bug", &["tests/**"], false);
        }

        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        );
        comment.file_path = PathBuf::from("tests/unit/parser.rs");

        let result = apply_feedback_confidence_adjustment(vec![comment], &feedback, 5);
        assert!(
            (result[0].confidence - 0.6).abs() < 0.01,
            "Expected path-scoped rejection history to win, got {}",
            result[0].confidence
        );
    }

    #[test]
    fn feedback_confidence_clamped_to_one() {
        let mut feedback = FeedbackStore::default();
        for _ in 0..10 {
            feedback.record_feedback("Bug", None, true);
        }

        // Even with high confidence, should not exceed 1.0
        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            1.0,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        assert!(result[0].confidence <= 1.0);
    }

    #[test]
    fn feedback_confidence_uses_file_pattern_fallback() {
        let mut feedback = FeedbackStore::default();
        let patterns = vec!["*.test.ts".to_string(), "*.ts".to_string()];
        for _ in 0..10 {
            feedback.record_feedback_patterns("Bug", &patterns, false);
        }

        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.8,
        );
        comment.file_path = PathBuf::from("web/src/Settings.test.ts");

        let result = apply_feedback_confidence_adjustment(vec![comment], &feedback, 5);
        assert!(
            (result[0].confidence - 0.6).abs() < 0.01,
            "Expected file-pattern fallback to demote confidence, got {}",
            result[0].confidence
        );
    }

    #[test]
    fn feedback_confidence_uses_rule_specific_stats_before_category_stats() {
        let mut feedback = FeedbackStore::default();
        for _ in 0..10 {
            feedback.record_feedback("Security", None, true);
            feedback.record_rule_feedback_patterns("sec.sql.injection", &["*.rs"], false);
        }

        let mut comment = build_comment(
            "c1",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.8,
        );
        comment.rule_id = Some("sec.sql.injection".to_string());

        let result = apply_feedback_confidence_adjustment(vec![comment], &feedback, 5);
        assert!(
            (result[0].confidence - 0.6).abs() < 0.01,
            "Expected rule-level rejection history to win, got {}",
            result[0].confidence
        );
    }

    #[test]
    fn feedback_confidence_boosts_exactly_accepted_comment_ids() {
        let mut feedback = FeedbackStore::default();
        feedback.accept.insert("c1".to_string());

        let comments = vec![build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.8,
        )];
        let result = apply_feedback_confidence_adjustment(comments, &feedback, 5);
        assert!(
            (result[0].confidence - 0.92).abs() < 0.01,
            "Expected exact accepted comment ids to get a boost, got {}",
            result[0].confidence
        );
        assert!(result[0].tags.contains(&"feedback-calibration".to_string()));
        assert!(result[0]
            .tags
            .contains(&"feedback-calibration:accepted-id".to_string()));
    }
}
