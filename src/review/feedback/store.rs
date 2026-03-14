use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

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
    /// Feedback stats keyed by normalized rule id (e.g., "sec.sql.injection").
    #[serde(default)]
    pub by_rule: HashMap<String, FeedbackPatternStats>,
    /// Feedback stats keyed by composite "rule_id|*.ext".
    #[serde(default)]
    pub by_rule_file_pattern: HashMap<String, FeedbackPatternStats>,
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
        let cat_stats = self.by_category.entry(category.to_string()).or_default();
        update_pattern_stats(cat_stats, accepted);

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

            let composite = format!("{}|{}", category, pattern);
            let comp_stats = self.by_category_file_pattern.entry(composite).or_default();
            update_pattern_stats(comp_stats, accepted);
        }
    }

    /// Record a feedback event across normalized rule-id buckets.
    pub fn record_rule_feedback_patterns<S>(
        &mut self,
        rule_id: &str,
        file_patterns: &[S],
        accepted: bool,
    ) where
        S: AsRef<str>,
    {
        let Some(rule_id) = normalize_feedback_key(rule_id) else {
            return;
        };

        let rule_stats = self.by_rule.entry(rule_id.clone()).or_default();
        update_pattern_stats(rule_stats, accepted);

        for pattern in collect_unique_patterns(file_patterns) {
            let composite = format!("{}|{}", rule_id, pattern);
            let comp_stats = self.by_rule_file_pattern.entry(composite).or_default();
            update_pattern_stats(comp_stats, accepted);
        }
    }
}

fn update_pattern_stats(stats: &mut FeedbackPatternStats, accepted: bool) {
    if accepted {
        stats.accepted += 1;
    } else {
        stats.rejected += 1;
    }
}

fn collect_unique_patterns<S>(file_patterns: &[S]) -> HashSet<String>
where
    S: AsRef<str>,
{
    let mut unique_patterns = HashSet::new();
    for pattern in file_patterns {
        let pattern = pattern.as_ref().trim();
        if pattern.is_empty() {
            continue;
        }
        unique_patterns.insert(pattern.to_string());
    }
    unique_patterns
}

fn normalize_feedback_key(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}
