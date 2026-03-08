use std::collections::HashSet;
use tracing::info;

use crate::config;
use crate::core;
use super::feedback::FeedbackStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewCommentType {
    Logic,
    Syntax,
    Style,
    Informational,
}

impl ReviewCommentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logic => "logic",
            Self::Syntax => "syntax",
            Self::Style => "style",
            Self::Informational => "informational",
        }
    }
}

pub fn classify_comment_type(comment: &core::Comment) -> ReviewCommentType {
    if matches!(comment.category, core::comment::Category::Style) {
        return ReviewCommentType::Style;
    }

    if matches!(
        comment.category,
        core::comment::Category::Documentation | core::comment::Category::BestPractice
    ) {
        return ReviewCommentType::Informational;
    }

    let content = comment.content.to_lowercase();
    if content.contains("syntax")
        || content.contains("parse error")
        || content.contains("compilation")
        || content.contains("compile")
        || content.contains("token")
    {
        return ReviewCommentType::Syntax;
    }

    ReviewCommentType::Logic
}

pub fn apply_comment_type_filter(
    comments: Vec<core::Comment>,
    enabled_types: &[String],
) -> Vec<core::Comment> {
    if enabled_types.is_empty() {
        return comments;
    }

    let enabled: HashSet<&str> = enabled_types.iter().map(String::as_str).collect();
    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        let comment_type = classify_comment_type(&comment);
        if enabled.contains(comment_type.as_str()) {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) due to comment type filters [{}]",
            dropped,
            enabled_types.join(", ")
        );
    }

    kept
}

pub fn should_adaptively_suppress(comment: &core::Comment, feedback: &FeedbackStore) -> bool {
    if matches!(
        comment.severity,
        core::comment::Severity::Error | core::comment::Severity::Warning
    ) {
        return false;
    }

    let key = classify_comment_type(comment).as_str();
    let stats = match feedback.by_comment_type.get(key) {
        Some(stats) => stats,
        None => return false,
    };

    stats.rejected >= 3 && stats.rejected >= stats.accepted.saturating_add(2)
}

pub fn apply_feedback_suppression(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    if feedback.suppress.is_empty() && feedback.by_comment_type.is_empty() {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);
    let mut explicit_dropped = 0usize;
    let mut adaptive_dropped = 0usize;

    for comment in comments {
        if feedback.suppress.contains(&comment.id) {
            explicit_dropped += 1;
            continue;
        }
        if should_adaptively_suppress(&comment, feedback) {
            adaptive_dropped += 1;
            continue;
        }
        kept.push(comment);
    }

    if explicit_dropped > 0 {
        info!(
            "Dropped {} comment(s) due to explicit feedback suppression rules",
            explicit_dropped
        );
    }
    if adaptive_dropped > 0 {
        info!(
            "Dropped {} low-priority comment(s) due to learned feedback preferences",
            adaptive_dropped
        );
    }

    kept
}

pub fn apply_confidence_threshold(
    comments: Vec<core::Comment>,
    min_confidence: f32,
) -> Vec<core::Comment> {
    if min_confidence <= 0.0 {
        return comments;
    }

    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        if comment.confidence >= min_confidence {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) below confidence threshold {}",
            dropped, min_confidence
        );
    }

    kept
}

pub fn apply_review_filters(
    comments: Vec<core::Comment>,
    config: &config::Config,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    let comments = apply_confidence_threshold(comments, config.effective_min_confidence());
    let comments = apply_comment_type_filter(comments, &config.comment_types);
    apply_feedback_suppression(comments, feedback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
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
            build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.9),
            build_comment("c2", core::comment::Category::Style, core::comment::Severity::Info, 0.8),
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
                },
            )]),
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

        let filtered = apply_feedback_suppression(comments, &feedback);

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
                },
            )]),
        };

        let comment = build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.9);
        assert!(!should_adaptively_suppress(&comment, &feedback));
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
                },
            )]),
        };

        let comment = build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Warning, 0.9);
        assert!(!should_adaptively_suppress(&comment, &feedback));
    }

    #[test]
    fn confidence_threshold_filters_low_confidence() {
        let comments = vec![
            build_comment("high", core::comment::Category::Bug, core::comment::Severity::Error, 0.9),
            build_comment("low", core::comment::Category::Bug, core::comment::Severity::Info, 0.3),
        ];
        let filtered = apply_confidence_threshold(comments, 0.5);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "high");
    }

    #[test]
    fn confidence_threshold_zero_keeps_all() {
        let comments = vec![
            build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.1),
        ];
        let filtered = apply_confidence_threshold(comments, 0.0);
        assert_eq!(filtered.len(), 1);
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
        let comment = build_comment("c1", core::comment::Category::Style, core::comment::Severity::Info, 0.9);
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Style);
    }

    #[test]
    fn classify_comment_type_informational() {
        let comment = build_comment("c1", core::comment::Category::Documentation, core::comment::Severity::Info, 0.9);
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Informational);
    }

    #[test]
    fn classify_comment_type_syntax() {
        let mut comment = build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.9);
        comment.content = "This has a syntax error".to_string();
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Syntax);
    }

    #[test]
    fn classify_comment_type_logic_default() {
        let comment = build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.9);
        assert_eq!(classify_comment_type(&comment), ReviewCommentType::Logic);
    }

    #[test]
    fn explicit_feedback_suppression() {
        let mut feedback = FeedbackStore::default();
        feedback.suppress.insert("c1".to_string());

        let comments = vec![
            build_comment("c1", core::comment::Category::Bug, core::comment::Severity::Error, 0.9),
            build_comment("c2", core::comment::Category::Bug, core::comment::Severity::Error, 0.9),
        ];

        let filtered = apply_feedback_suppression(comments, &feedback);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "c2");
    }
}
