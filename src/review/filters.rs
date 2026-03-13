use std::collections::HashSet;
use tracing::info;

use super::feedback::{derive_file_patterns, FeedbackStore};
use crate::config;
use crate::core;

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

const VAGUE_COMMENT_PREFIXES: &[&str] = &[
    "ensure",
    "verify",
    "validate",
    "consider",
    "review",
    "confirm",
    "check",
    "make sure",
];

const VAGUE_COMMENT_PHRASES: &[&str] = &[
    "ensure that",
    "verify that",
    "validate that",
    "consider adding",
    "consider using",
    "make sure",
    "double-check",
    "it may be worth",
];

pub fn is_vague_comment_text(text: &str) -> bool {
    let trimmed = text
        .trim()
        .trim_start_matches(|ch: char| ch == '-' || ch == '*' || ch == ':' || ch.is_whitespace())
        .trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if VAGUE_COMMENT_PREFIXES
        .iter()
        .any(|prefix| lower == *prefix || lower.starts_with(&format!("{} ", prefix)))
    {
        return true;
    }

    VAGUE_COMMENT_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn is_vague_review_comment(comment: &core::Comment) -> bool {
    is_vague_comment_text(&comment.content)
}

pub fn apply_vague_comment_filter(comments: Vec<core::Comment>) -> Vec<core::Comment> {
    let total = comments.len();
    let kept: Vec<_> = comments
        .into_iter()
        .filter(|comment| !is_vague_review_comment(comment))
        .collect();

    if kept.len() != total {
        info!(
            "Dropped {} vague review comment(s) after generation",
            total.saturating_sub(kept.len())
        );
    }

    kept
}

pub fn should_adaptively_suppress_with_thresholds(
    comment: &core::Comment,
    feedback: &FeedbackStore,
    rejected_threshold: usize,
    margin: usize,
) -> bool {
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

    stats.rejected >= rejected_threshold && stats.rejected >= stats.accepted.saturating_add(margin)
}

pub fn apply_feedback_suppression_with_thresholds(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
    rejected_threshold: usize,
    margin: usize,
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
        if should_adaptively_suppress_with_thresholds(
            &comment,
            feedback,
            rejected_threshold,
            margin,
        ) {
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

/// Adjust comment confidence scores based on historical feedback acceptance rates.
///
/// For each comment, looks up the most specific composite key
/// (e.g. `Bug|*.test.ts`) first, then the matching file-pattern bucket,
/// then falls back to category-only. If enough observations exist,
/// adjusts confidence: `confidence *= (0.5 + acceptance_rate * 0.5)`.
/// This maps 0% acceptance → 0.5x, 100% acceptance → 1.0x.
pub fn apply_feedback_confidence_adjustment(
    comments: Vec<core::Comment>,
    feedback: &FeedbackStore,
    min_observations: usize,
) -> Vec<core::Comment> {
    comments
        .into_iter()
        .map(|mut comment| {
            let category = comment.category.to_string();
            let file_patterns = derive_file_patterns(&comment.file_path);

            // Try the most specific composite key first, then file-pattern-only,
            // then category-only.
            let stats = file_patterns
                .iter()
                .find_map(|pattern| {
                    let key = format!("{}|{}", category, pattern);
                    feedback.by_category_file_pattern.get(&key)
                })
                .or_else(|| {
                    file_patterns
                        .iter()
                        .find_map(|pattern| feedback.by_file_pattern.get(pattern))
                })
                .or_else(|| feedback.by_category.get(&category));

            if let Some(stats) = stats {
                if stats.total() >= min_observations {
                    let rate = stats.acceptance_rate();
                    let adjustment = 0.5 + rate * 0.5;
                    comment.confidence = (comment.confidence * adjustment).clamp(0.0, 1.0);
                }
            }

            comment
        })
        .collect()
}

pub fn apply_review_filters(
    comments: Vec<core::Comment>,
    config: &config::Config,
    feedback: &FeedbackStore,
) -> Vec<core::Comment> {
    let comments = apply_confidence_threshold(comments, config.effective_min_confidence());
    let comments = apply_comment_type_filter(comments, &config.comment_types);
    let comments = apply_vague_comment_filter(comments);
    apply_feedback_suppression_with_thresholds(
        comments,
        feedback,
        config.feedback_suppression_threshold,
        config.feedback_suppression_margin,
    )
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
    fn feedback_confidence_zero_acceptance_halves() {
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
        // 0% acceptance → 0.5 * 0.8 = 0.4
        assert!(
            (result[0].confidence - 0.4).abs() < 0.01,
            "Got: {}",
            result[0].confidence
        );
    }

    #[test]
    fn feedback_confidence_full_acceptance_unchanged() {
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
        // 100% acceptance → 1.0 * 0.8 = 0.8
        assert!(
            (result[0].confidence - 0.8).abs() < 0.01,
            "Got: {}",
            result[0].confidence
        );
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
        // Composite key should take precedence: 0% → 0.5 * 0.8 = 0.4
        // Note: the composite stats include the category-only accepts too,
        // so composite has accepted=0, rejected=10. Category has accepted=10+0=10, rejected=0+10=10
        // Actually record_feedback("Bug", Some("*.rs"), false) adds to by_category too
        // So by_category["Bug"] = accepted:10, rejected:10 = 50% acceptance
        // by_category_file_pattern["Bug|*.rs"] = accepted:0, rejected:10 = 0%
        // Composite wins: 0.5 * 0.8 = 0.4
        assert!(
            (result[0].confidence - 0.4).abs() < 0.01,
            "Got: {}",
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
            (result[0].confidence - 0.4).abs() < 0.01,
            "Expected file-pattern fallback to halve confidence, got {}",
            result[0].confidence
        );
    }
}
