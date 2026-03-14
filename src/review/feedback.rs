#[path = "feedback/context.rs"]
mod context;
#[path = "feedback/patterns.rs"]
mod patterns;
#[path = "feedback/persistence.rs"]
mod persistence;
#[path = "feedback/record.rs"]
mod record;
#[path = "feedback/semantic.rs"]
mod semantic;
#[path = "feedback/store.rs"]
mod store;

#[allow(unused_imports)]
pub use context::generate_feedback_context;
#[allow(unused_imports)]
pub use patterns::derive_file_patterns;
#[allow(unused_imports)]
pub use persistence::{load_feedback_store, load_feedback_store_from_path, save_feedback_store};
#[allow(unused_imports)]
pub use record::{apply_comment_feedback_signal, record_comment_feedback_stats};
#[allow(unused_imports)]
pub use semantic::{record_semantic_feedback_example, record_semantic_feedback_examples};
#[allow(unused_imports)]
pub use store::{FeedbackPatternStats, FeedbackStore, FeedbackTypeStats};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn feedback_store_default_is_empty() {
        let store = FeedbackStore::default();
        assert!(store.suppress.is_empty());
        assert!(store.accept.is_empty());
        assert!(store.by_comment_type.is_empty());
        assert!(store.by_category.is_empty());
        assert!(store.by_file_pattern.is_empty());
        assert!(store.by_category_file_pattern.is_empty());
        assert!(store.by_rule.is_empty());
        assert!(store.by_rule_file_pattern.is_empty());
    }

    #[test]
    fn derive_file_patterns_adds_path_scopes_before_suffixes() {
        let patterns = derive_file_patterns(Path::new("web/src/Settings.test.ts"));
        assert_eq!(
            patterns,
            vec!["web/src/**", "web/**", "src/**", "*.test.ts", "*.ts"]
        );
    }

    #[test]
    fn derive_file_patterns_supports_extensionless_path_scopes() {
        let patterns = derive_file_patterns(Path::new("scripts/release"));
        assert_eq!(patterns, vec!["scripts/**"]);
    }

    #[test]
    fn feedback_store_roundtrip_json() {
        let mut store = FeedbackStore::default();
        store.suppress.insert("c1".to_string());
        store.accept.insert("c2".to_string());
        store.by_comment_type.insert(
            "style".to_string(),
            FeedbackTypeStats {
                accepted: 1,
                rejected: 2,
            },
        );

        let json = serde_json::to_string(&store).unwrap();
        let deserialized: FeedbackStore = serde_json::from_str(&json).unwrap();
        assert!(deserialized.suppress.contains("c1"));
        assert!(deserialized.accept.contains("c2"));
        assert_eq!(deserialized.by_comment_type["style"].accepted, 1);
        assert_eq!(deserialized.by_comment_type["style"].rejected, 2);
    }

    #[test]
    fn load_feedback_store_from_nonexistent_path_returns_default() {
        let store = load_feedback_store_from_path(Path::new("/nonexistent/path.json"));
        assert!(store.suppress.is_empty());
    }

    // ── FeedbackPatternStats tests ────────────────────────────────────────

    #[test]
    fn pattern_stats_acceptance_rate_no_data() {
        let stats = FeedbackPatternStats::default();
        assert_eq!(stats.acceptance_rate(), 0.5); // neutral
        assert_eq!(stats.total(), 0);
    }

    #[test]
    fn pattern_stats_acceptance_rate_all_accepted() {
        let stats = FeedbackPatternStats {
            accepted: 10,
            rejected: 0,
        };
        assert_eq!(stats.acceptance_rate(), 1.0);
        assert_eq!(stats.total(), 10);
    }

    #[test]
    fn pattern_stats_acceptance_rate_all_rejected() {
        let stats = FeedbackPatternStats {
            accepted: 0,
            rejected: 10,
        };
        assert_eq!(stats.acceptance_rate(), 0.0);
    }

    #[test]
    fn pattern_stats_acceptance_rate_mixed() {
        let stats = FeedbackPatternStats {
            accepted: 3,
            rejected: 7,
        };
        assert!((stats.acceptance_rate() - 0.3).abs() < f32::EPSILON);
    }

    // ── record_feedback tests ─────────────────────────────────────────────

    #[test]
    fn record_feedback_category_only() {
        let mut store = FeedbackStore::default();
        store.record_feedback("Bug", None, true);
        store.record_feedback("Bug", None, false);
        store.record_feedback("Bug", None, true);

        let cat = &store.by_category["Bug"];
        assert_eq!(cat.accepted, 2);
        assert_eq!(cat.rejected, 1);
        assert!(store.by_file_pattern.is_empty());
        assert!(store.by_category_file_pattern.is_empty());
    }

    #[test]
    fn record_feedback_with_file_pattern() {
        let mut store = FeedbackStore::default();
        store.record_feedback("Security", Some("*.rs"), true);
        store.record_feedback("Security", Some("*.rs"), false);

        assert_eq!(store.by_category["Security"].accepted, 1);
        assert_eq!(store.by_category["Security"].rejected, 1);
        assert_eq!(store.by_file_pattern["*.rs"].accepted, 1);
        assert_eq!(store.by_file_pattern["*.rs"].rejected, 1);
        assert_eq!(store.by_category_file_pattern["Security|*.rs"].accepted, 1);
        assert_eq!(store.by_category_file_pattern["Security|*.rs"].rejected, 1);
    }

    #[test]
    fn record_feedback_with_multiple_file_patterns() {
        let mut store = FeedbackStore::default();
        let patterns = vec!["*.test.ts".to_string(), "*.ts".to_string()];

        store.record_feedback_patterns("Bug", &patterns, false);

        assert_eq!(store.by_category["Bug"].accepted, 0);
        assert_eq!(store.by_category["Bug"].rejected, 1);
        assert_eq!(store.by_file_pattern["*.test.ts"].rejected, 1);
        assert_eq!(store.by_file_pattern["*.ts"].rejected, 1);
        assert_eq!(store.by_category_file_pattern["Bug|*.test.ts"].rejected, 1);
        assert_eq!(store.by_category_file_pattern["Bug|*.ts"].rejected, 1);
    }

    #[test]
    fn record_feedback_roundtrip_json() {
        let mut store = FeedbackStore::default();
        for _ in 0..5 {
            store.record_feedback("Bug", Some("*.ts"), true);
        }
        for _ in 0..3 {
            store.record_feedback("Bug", Some("*.ts"), false);
        }

        let json = serde_json::to_string(&store).unwrap();
        let deserialized: FeedbackStore = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.by_category["Bug"].accepted, 5);
        assert_eq!(deserialized.by_category["Bug"].rejected, 3);
        assert_eq!(deserialized.by_file_pattern["*.ts"].total(), 8);
        assert_eq!(deserialized.by_category_file_pattern["Bug|*.ts"].total(), 8);
        assert!(deserialized.by_rule.is_empty());
        assert!(deserialized.by_rule_file_pattern.is_empty());
    }

    // ── generate_feedback_context tests ───────────────────────────────────

    #[test]
    fn generate_feedback_context_empty_store() {
        let store = FeedbackStore::default();
        assert!(generate_feedback_context(&store).is_empty());
    }

    #[test]
    fn generate_feedback_context_insufficient_data() {
        let mut store = FeedbackStore::default();
        // Only 3 observations, below the threshold of 5
        for _ in 0..3 {
            store.record_feedback("Bug", None, false);
        }
        assert!(generate_feedback_context(&store).is_empty());
    }

    #[test]
    fn generate_feedback_context_high_acceptance() {
        let mut store = FeedbackStore::default();
        for _ in 0..8 {
            store.record_feedback("Security", None, true);
        }
        for _ in 0..2 {
            store.record_feedback("Security", None, false);
        }
        let context = generate_feedback_context(&store);
        assert!(
            context.contains("Security"),
            "Should mention Security: {}",
            context
        );
        assert!(
            context.contains("thorough"),
            "Should advise thoroughness: {}",
            context
        );
    }

    #[test]
    fn generate_feedback_context_low_acceptance() {
        let mut store = FeedbackStore::default();
        for _ in 0..1 {
            store.record_feedback("Style", None, true);
        }
        for _ in 0..9 {
            store.record_feedback("Style", None, false);
        }
        let context = generate_feedback_context(&store);
        assert!(
            context.contains("Style"),
            "Should mention Style: {}",
            context
        );
        assert!(
            context.contains("rejected"),
            "Should note rejection: {}",
            context
        );
    }

    #[test]
    fn generate_feedback_context_file_pattern_low_acceptance() {
        let mut store = FeedbackStore::default();
        for _ in 0..1 {
            store.record_feedback("Bug", Some("*.test.ts"), true);
        }
        for _ in 0..9 {
            store.record_feedback("Bug", Some("*.test.ts"), false);
        }
        let context = generate_feedback_context(&store);
        assert!(
            context.contains("*.test.ts"),
            "Should mention file pattern: {}",
            context
        );
    }
}
