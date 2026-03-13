use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::adapters;
use crate::config;
use crate::core;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackTypeStats {
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
}

/// Tracks acceptance/rejection counts for a specific pattern (category, file extension, etc.)
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FeedbackPatternStats {
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
}

impl FeedbackPatternStats {
    pub fn acceptance_rate(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            return 0.5; // neutral when no data
        }
        self.accepted as f32 / total as f32
    }

    pub fn total(&self) -> usize {
        self.accepted + self.rejected
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackStore {
    #[serde(default)]
    pub suppress: HashSet<String>,
    #[serde(default)]
    pub accept: HashSet<String>,
    #[serde(default)]
    pub by_comment_type: HashMap<String, FeedbackTypeStats>,
    /// Feedback stats keyed by category (e.g., "Bug", "Security", "Performance").
    #[serde(default)]
    pub by_category: HashMap<String, FeedbackPatternStats>,
    /// Feedback stats keyed by file extension glob (e.g., "*.rs", "*.test.ts").
    #[serde(default)]
    pub by_file_pattern: HashMap<String, FeedbackPatternStats>,
    /// Feedback stats keyed by composite "category|*.ext" (e.g., "Bug|*.rs").
    #[serde(default)]
    pub by_category_file_pattern: HashMap<String, FeedbackPatternStats>,
}

pub fn derive_file_patterns(path: &Path) -> Vec<String> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };

    let parts: Vec<&str> = file_name.split('.').collect();
    if parts.len() < 2 {
        return Vec::new();
    }

    let mut patterns = Vec::new();
    for start in 1..parts.len() {
        let pattern = format!("*.{}", parts[start..].join("."));
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }

    patterns
}

fn update_pattern_stats(stats: &mut FeedbackPatternStats, accepted: bool) {
    if accepted {
        stats.accepted += 1;
    } else {
        stats.rejected += 1;
    }
}

impl FeedbackStore {
    /// Record a feedback event for enhanced pattern tracking.
    #[cfg(test)]
    pub fn record_feedback(&mut self, category: &str, file_pattern: Option<&str>, accepted: bool) {
        let patterns = file_pattern
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        self.record_feedback_patterns(category, &patterns, accepted);
    }

    /// Record a feedback event across one or more file-pattern buckets.
    pub fn record_feedback_patterns<S>(
        &mut self,
        category: &str,
        file_patterns: &[S],
        accepted: bool,
    ) where
        S: AsRef<str>,
    {
        // Update by_category
        let cat_stats = self.by_category.entry(category.to_string()).or_default();
        update_pattern_stats(cat_stats, accepted);

        // Update by_file_pattern
        let mut unique_patterns = HashSet::new();
        for pattern in file_patterns {
            let pattern = pattern.as_ref().trim();
            if pattern.is_empty() {
                continue;
            }
            unique_patterns.insert(pattern.to_string());
        }

        for pattern in unique_patterns {
            let fp_stats = self.by_file_pattern.entry(pattern.clone()).or_default();
            update_pattern_stats(fp_stats, accepted);

            // Update composite key
            let composite = format!("{}|{}", category, pattern);
            let comp_stats = self.by_category_file_pattern.entry(composite).or_default();
            update_pattern_stats(comp_stats, accepted);
        }
    }
}

pub fn load_feedback_store_from_path(path: &Path) -> FeedbackStore {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => FeedbackStore::default(),
    }
}

pub fn load_feedback_store(config: &config::Config) -> FeedbackStore {
    load_feedback_store_from_path(&config.feedback_path)
}

/// Generate feedback context to inject into the review prompt.
///
/// Scans the feedback store for statistically significant patterns
/// and generates guidance text for the LLM reviewer.
pub fn generate_feedback_context(store: &FeedbackStore) -> String {
    let min_observations = 5;
    let mut patterns: Vec<String> = Vec::new();

    // Scan by_category for significant patterns
    for (category, stats) in &store.by_category {
        if stats.total() < min_observations {
            continue;
        }
        let rate = stats.acceptance_rate();
        if rate >= 0.7 {
            patterns.push(format!(
                "- {} findings are usually accepted ({:.0}% acceptance rate) — be thorough on {} issues",
                category, rate * 100.0, category.to_lowercase()
            ));
        } else if rate < 0.3 {
            patterns.push(format!(
                "- {} findings are frequently rejected ({:.0}% acceptance rate) — only flag clear {} issues",
                category, rate * 100.0, category.to_lowercase()
            ));
        }
    }

    // Scan by_file_pattern for low-acceptance patterns
    for (pattern, stats) in &store.by_file_pattern {
        if stats.total() < min_observations {
            continue;
        }
        let rate = stats.acceptance_rate();
        if rate < 0.3 {
            patterns.push(format!(
                "- Comments on {} files are usually rejected ({:.0}% acceptance rate) — be more conservative",
                pattern, rate * 100.0
            ));
        }
    }

    // Cap at top 10 patterns to avoid prompt bloat
    patterns.truncate(10);

    if patterns.is_empty() {
        return String::new();
    }

    let mut context = String::from(
        "## Learned Feedback Patterns\nBased on historical feedback from this project:\n",
    );
    for pattern in &patterns {
        context.push_str(pattern);
        context.push('\n');
    }
    context
}

pub fn save_feedback_store(path: &Path, store: &FeedbackStore) -> Result<()> {
    atomic_write_string(path, &serde_json::to_string_pretty(store)?)
}

#[allow(dead_code)]
pub async fn record_semantic_feedback_example(
    config: &config::Config,
    comment: &core::Comment,
    accepted: bool,
) -> Result<()> {
    record_semantic_feedback_examples(config, std::slice::from_ref(comment), accepted).await?;
    Ok(())
}

pub async fn record_semantic_feedback_examples(
    config: &config::Config,
    comments: &[core::Comment],
    accepted: bool,
) -> Result<usize> {
    if comments.is_empty() {
        return Ok(0);
    }

    let semantic_path = core::default_semantic_feedback_path(&config.feedback_path);
    let mut store = core::load_semantic_feedback_store(&semantic_path);
    let model_config = config.to_model_config_for_role(config::ModelRole::Embedding);
    let adapter = adapters::llm::create_adapter(&model_config).ok();
    core::align_semantic_feedback_store(&mut store, adapter.as_deref());

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(adapter.as_deref(), &embedding_texts).await;
    let before = store.examples.len();
    let timestamp = chrono::Utc::now().to_rfc3339();

    for (comment, embedding) in comments.iter().zip(embeddings.into_iter()) {
        store.add_example(core::SemanticFeedbackExample {
            content: comment.content.clone(),
            category: comment.category.as_str().to_string(),
            file_patterns: derive_file_patterns(&comment.file_path),
            accepted,
            created_at: timestamp.clone(),
            embedding,
        });
    }

    core::save_semantic_feedback_store(&semantic_path, &store)?;
    Ok(store.examples.len().saturating_sub(before))
}

fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("feedback.json");
    let tmp_path = path.with_file_name(format!("{}.{}.tmp", file_name, std::process::id()));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_store_default_is_empty() {
        let store = FeedbackStore::default();
        assert!(store.suppress.is_empty());
        assert!(store.accept.is_empty());
        assert!(store.by_comment_type.is_empty());
        assert!(store.by_category.is_empty());
        assert!(store.by_file_pattern.is_empty());
        assert!(store.by_category_file_pattern.is_empty());
    }

    #[test]
    fn derive_file_patterns_prefers_specific_suffixes() {
        let patterns = derive_file_patterns(Path::new("web/src/Settings.test.ts"));
        assert_eq!(patterns, vec!["*.test.ts", "*.ts"]);
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
