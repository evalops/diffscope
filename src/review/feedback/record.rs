use crate::core;

use super::super::filters::classify_comment_type;
use super::super::rule_helpers::normalize_rule_id;
use super::patterns::derive_file_patterns;
use super::store::FeedbackStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentResolutionOutcome {
    Addressed,
    NotAddressed,
}

pub fn record_comment_feedback_stats(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    if accepted {
        stats.accepted = stats.accepted.saturating_add(1);
        stats.accepted_weight += 1.0;
    } else {
        stats.rejected = stats.rejected.saturating_add(1);
        stats.rejected_weight += 1.0;
    }

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, accepted);
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_feedback_patterns(&rule_id, &file_patterns, accepted);
    }
}

pub fn record_comment_feedback_stats_with_weight(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
    weight: f32,
) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    if accepted {
        stats.accepted = stats.accepted.saturating_add(1);
        stats.accepted_weight += weight;
    } else {
        stats.rejected = stats.rejected.saturating_add(1);
        stats.rejected_weight += weight;
    }

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_feedback_patterns_with_weight(
        &comment.category.to_string(),
        &file_patterns,
        accepted,
        weight,
    );
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_feedback_patterns_at_weighted(
            &rule_id,
            &file_patterns,
            accepted,
            weight,
            chrono::Utc::now().timestamp(),
        );
    }
}

pub fn record_comment_feedback_stats_at(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
    timestamp: i64,
) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    if accepted {
        stats.accepted = stats.accepted.saturating_add(1);
        stats.accepted_weight += 1.0;
    } else {
        stats.rejected = stats.rejected.saturating_add(1);
        stats.rejected_weight += 1.0;
    }

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_feedback_patterns(&comment.category.to_string(), &file_patterns, accepted);
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_feedback_patterns_at(&rule_id, &file_patterns, accepted, timestamp);
    }
}

pub fn record_comment_dismissal_stats(store: &mut FeedbackStore, comment: &core::Comment) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    stats.dismissed = stats.dismissed.saturating_add(1);
    stats.dismissed_weight += 1.0;

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_dismissal_patterns(&comment.category.to_string(), &file_patterns);
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_dismissal_patterns(&rule_id, &file_patterns);
    }
}

pub fn record_comment_resolution_stats(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    outcome: CommentResolutionOutcome,
) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    let addressed = matches!(outcome, CommentResolutionOutcome::Addressed);

    if addressed {
        stats.addressed = stats.addressed.saturating_add(1);
        stats.addressed_weight += 1.0;
    } else {
        stats.not_addressed = stats.not_addressed.saturating_add(1);
        stats.not_addressed_weight += 1.0;
    }

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_outcome_patterns(&comment.category.to_string(), &file_patterns, addressed);
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_outcome_patterns(&rule_id, &file_patterns, addressed);
    }
}

pub fn record_comment_resolution_stats_at(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    outcome: CommentResolutionOutcome,
    timestamp: i64,
) {
    let key = classify_comment_type(comment).as_str().to_string();
    let stats = store.by_comment_type.entry(key).or_default();
    let addressed = matches!(outcome, CommentResolutionOutcome::Addressed);

    if addressed {
        stats.addressed = stats.addressed.saturating_add(1);
        stats.addressed_weight += 1.0;
    } else {
        stats.not_addressed = stats.not_addressed.saturating_add(1);
        stats.not_addressed_weight += 1.0;
    }

    let file_patterns = derive_file_patterns(&comment.file_path);
    store.record_outcome_patterns(&comment.category.to_string(), &file_patterns, addressed);
    if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
        store.record_rule_outcome_patterns_at(&rule_id, &file_patterns, addressed, timestamp);
    }
}

pub fn apply_comment_feedback_signal(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
) -> bool {
    let changed = if accepted {
        store.suppress.remove(&comment.id);
        store.accept.insert(comment.id.clone())
    } else {
        store.accept.remove(&comment.id);
        store.suppress.insert(comment.id.clone())
    };

    if changed {
        record_comment_feedback_stats(store, comment, accepted);
    }

    changed
}

pub fn apply_comment_feedback_signal_with_weight(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
    weight: f32,
) -> bool {
    let changed = if accepted {
        store.suppress.remove(&comment.id);
        store.accept.insert(comment.id.clone())
    } else {
        store.accept.remove(&comment.id);
        store.suppress.insert(comment.id.clone())
    };

    if changed {
        record_comment_feedback_stats_with_weight(store, comment, accepted, weight);
    }

    changed
}

pub fn apply_comment_feedback_signal_at(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    accepted: bool,
    timestamp: i64,
) -> bool {
    let changed = if accepted {
        store.suppress.remove(&comment.id);
        store.accept.insert(comment.id.clone())
    } else {
        store.accept.remove(&comment.id);
        store.suppress.insert(comment.id.clone())
    };

    if changed {
        record_comment_feedback_stats_at(store, comment, accepted, timestamp);
    }

    changed
}

pub fn apply_comment_dismissal_signal(store: &mut FeedbackStore, comment: &core::Comment) -> bool {
    let changed = store.dismissed.insert(comment.id.clone());

    if changed {
        record_comment_dismissal_stats(store, comment);
    }

    changed
}

pub fn apply_comment_resolution_outcome_signal(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    outcome: CommentResolutionOutcome,
) -> bool {
    let changed = match outcome {
        CommentResolutionOutcome::Addressed => store.addressed.insert(comment.id.clone()),
        CommentResolutionOutcome::NotAddressed => store.not_addressed.insert(comment.id.clone()),
    };

    if changed {
        record_comment_resolution_stats(store, comment, outcome);
    }

    changed
}

pub fn apply_comment_resolution_outcome_signal_at(
    store: &mut FeedbackStore,
    comment: &core::Comment,
    outcome: CommentResolutionOutcome,
    timestamp: i64,
) -> bool {
    let changed = match outcome {
        CommentResolutionOutcome::Addressed => store.addressed.insert(comment.id.clone()),
        CommentResolutionOutcome::NotAddressed => store.not_addressed.insert(comment.id.clone()),
    };

    if changed {
        record_comment_resolution_stats_at(store, comment, outcome, timestamp);
    }

    changed
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sample_comment() -> core::Comment {
        core::Comment {
            id: "cmt_sql".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "The query is still vulnerable to SQL injection.".to_string(),
            rule_id: Some(" SEC.SQL.INJECTION ".to_string()),
            severity: core::comment::Severity::Error,
            category: core::comment::Category::Security,
            suggestion: None,
            confidence: 0.82,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Medium,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn apply_comment_feedback_signal_is_idempotent_for_repeat_actions() {
        let comment = sample_comment();
        let mut store = FeedbackStore::default();

        assert!(apply_comment_feedback_signal_at(
            &mut store, &comment, true, 1_000
        ));
        assert!(!apply_comment_feedback_signal_at(
            &mut store, &comment, true, 1_000,
        ));

        assert!(store.accept.contains(&comment.id));
        assert_eq!(store.by_category["Security"].accepted, 1);
        assert_eq!(store.by_file_pattern["src/**"].accepted, 1);
        assert_eq!(
            store.by_category_file_pattern["Security|src/**"].accepted,
            1
        );
        assert_eq!(store.by_rule["sec.sql.injection"].accepted, 1);
        assert_eq!(
            store.by_rule_file_pattern["sec.sql.injection|src/**"].accepted,
            1
        );
        assert_eq!(
            store.by_rule_file_pattern["sec.sql.injection|*.rs"].accepted,
            1
        );
        assert_eq!(
            store.by_rule["sec.sql.injection"].decayed_total_at(1_000),
            Some(1.0)
        );
    }

    #[test]
    fn apply_comment_feedback_signal_records_direction_changes_once_per_state() {
        let comment = sample_comment();
        let mut store = FeedbackStore::default();

        assert!(apply_comment_feedback_signal(&mut store, &comment, true));
        assert!(apply_comment_feedback_signal(&mut store, &comment, false));
        assert!(!apply_comment_feedback_signal(&mut store, &comment, false));

        assert!(!store.accept.contains(&comment.id));
        assert!(store.suppress.contains(&comment.id));
        assert_eq!(store.by_category["Security"].accepted, 1);
        assert_eq!(store.by_category["Security"].rejected, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].accepted, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].rejected, 1);
    }

    #[test]
    fn apply_comment_dismissal_signal_is_idempotent() {
        let comment = sample_comment();
        let mut store = FeedbackStore::default();

        assert!(apply_comment_dismissal_signal(&mut store, &comment));
        assert!(!apply_comment_dismissal_signal(&mut store, &comment));

        assert!(store.dismissed.contains(&comment.id));
        assert_eq!(store.by_comment_type["logic"].dismissed, 1);
        assert_eq!(store.by_category["Security"].dismissed, 1);
        assert_eq!(store.by_file_pattern["src/**"].dismissed, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].dismissed, 1);
        assert_eq!(
            store.by_rule_file_pattern["sec.sql.injection|*.rs"].dismissed,
            1
        );
    }

    #[test]
    fn apply_comment_resolution_outcome_signal_is_idempotent_per_outcome() {
        let comment = sample_comment();
        let mut store = FeedbackStore::default();

        assert!(apply_comment_resolution_outcome_signal(
            &mut store,
            &comment,
            CommentResolutionOutcome::Addressed,
        ));
        assert!(!apply_comment_resolution_outcome_signal(
            &mut store,
            &comment,
            CommentResolutionOutcome::Addressed,
        ));

        assert!(store.addressed.contains(&comment.id));
        assert_eq!(store.by_comment_type["logic"].addressed, 1);
        assert_eq!(store.by_category["Security"].addressed, 1);
        assert_eq!(store.by_file_pattern["src/**"].addressed, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].addressed, 1);
    }

    #[test]
    fn apply_comment_resolution_outcome_signal_records_both_outcome_directions_once() {
        let comment = sample_comment();
        let mut store = FeedbackStore::default();

        assert!(apply_comment_resolution_outcome_signal(
            &mut store,
            &comment,
            CommentResolutionOutcome::NotAddressed,
        ));
        assert!(apply_comment_resolution_outcome_signal(
            &mut store,
            &comment,
            CommentResolutionOutcome::Addressed,
        ));
        assert!(!apply_comment_resolution_outcome_signal(
            &mut store,
            &comment,
            CommentResolutionOutcome::NotAddressed,
        ));

        assert!(store.not_addressed.contains(&comment.id));
        assert!(store.addressed.contains(&comment.id));
        assert_eq!(store.by_comment_type["logic"].not_addressed, 1);
        assert_eq!(store.by_comment_type["logic"].addressed, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].not_addressed, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].addressed, 1);
    }
}
