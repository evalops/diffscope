use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

const RULE_REINFORCEMENT_DECAY_HALF_LIFE_SECS: f64 = 30.0 * 24.0 * 60.0 * 60.0;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackTypeStats {
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub dismissed: usize,
    #[serde(default)]
    pub addressed: usize,
    #[serde(default)]
    pub not_addressed: usize,
}

impl FeedbackTypeStats {
    pub fn positive_total(&self) -> usize {
        self.accepted + self.addressed
    }

    pub fn negative_total(&self) -> usize {
        self.rejected + self.not_addressed
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DecayedFeedbackStats {
    #[serde(default)]
    pub positive: f32,
    #[serde(default)]
    pub negative: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<i64>,
}

impl DecayedFeedbackStats {
    fn counts_at(&self, timestamp: i64) -> (f32, f32) {
        let Some(last_event_at) = self.last_event_at else {
            return (self.positive, self.negative);
        };

        let elapsed_secs = (timestamp - last_event_at).max(0) as f64;
        let decay = 0.5f64.powf(elapsed_secs / RULE_REINFORCEMENT_DECAY_HALF_LIFE_SECS) as f32;
        (self.positive * decay, self.negative * decay)
    }

    fn record_signal(&mut self, positive_signal: bool, timestamp: i64) {
        let (mut positive, mut negative) = self.counts_at(timestamp);
        if positive_signal {
            positive += 1.0;
        } else {
            negative += 1.0;
        }

        self.positive = positive;
        self.negative = negative;
        self.last_event_at = Some(timestamp);
    }
}

/// Tracks explicit and outcome-derived reinforcement counts for a specific pattern.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FeedbackPatternStats {
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub dismissed: usize,
    #[serde(default)]
    pub addressed: usize,
    #[serde(default)]
    pub not_addressed: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decayed: Option<DecayedFeedbackStats>,
}

impl FeedbackPatternStats {
    pub fn acceptance_rate(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            return 0.5; // neutral when no data
        }
        self.positive_total() as f32 / total as f32
    }

    pub fn total(&self) -> usize {
        self.positive_total() + self.negative_total()
    }

    pub fn positive_total(&self) -> usize {
        self.accepted + self.addressed
    }

    pub fn negative_total(&self) -> usize {
        self.rejected + self.not_addressed
    }

    pub fn decayed_acceptance_rate_at(&self, timestamp: i64) -> Option<f32> {
        let (positive, negative) = self.decayed_counts_at(timestamp)?;
        let total = positive + negative;
        if total <= f32::EPSILON {
            return Some(0.5);
        }
        Some(positive / total)
    }

    pub fn decayed_total_at(&self, timestamp: i64) -> Option<f32> {
        let (positive, negative) = self.decayed_counts_at(timestamp)?;
        Some(positive + negative)
    }

    pub fn record_decayed_signal(&mut self, positive_signal: bool, timestamp: i64) {
        self.decayed
            .get_or_insert_with(DecayedFeedbackStats::default)
            .record_signal(positive_signal, timestamp);
    }

    fn decayed_counts_at(&self, timestamp: i64) -> Option<(f32, f32)> {
        Some(self.decayed.as_ref()?.counts_at(timestamp))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeedbackStore {
    #[serde(default)]
    pub suppress: HashSet<String>,
    #[serde(default)]
    pub accept: HashSet<String>,
    #[serde(default)]
    pub dismissed: HashSet<String>,
    #[serde(default)]
    pub addressed: HashSet<String>,
    #[serde(default)]
    pub not_addressed: HashSet<String>,
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

            let composite = format!("{category}|{pattern}");
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
        self.record_rule_feedback_patterns_at(
            rule_id,
            file_patterns,
            accepted,
            current_timestamp(),
        );
    }

    /// Record a feedback event across normalized rule-id buckets at a specific timestamp.
    pub fn record_rule_feedback_patterns_at<S>(
        &mut self,
        rule_id: &str,
        file_patterns: &[S],
        accepted: bool,
        timestamp: i64,
    ) where
        S: AsRef<str>,
    {
        let Some(rule_id) = normalize_feedback_key(rule_id) else {
            return;
        };

        let rule_stats = self.by_rule.entry(rule_id.clone()).or_default();
        update_pattern_stats(rule_stats, accepted);
        rule_stats.record_decayed_signal(accepted, timestamp);

        for pattern in collect_unique_patterns(file_patterns) {
            let composite = format!("{}|{}", rule_id, pattern);
            let comp_stats = self.by_rule_file_pattern.entry(composite).or_default();
            update_pattern_stats(comp_stats, accepted);
            comp_stats.record_decayed_signal(accepted, timestamp);
        }
    }

    /// Record a dismissal event across one or more file-pattern buckets.
    pub fn record_dismissal_patterns<S>(&mut self, category: &str, file_patterns: &[S])
    where
        S: AsRef<str>,
    {
        let cat_stats = self.by_category.entry(category.to_string()).or_default();
        update_pattern_dismissed(cat_stats);

        for pattern in collect_unique_patterns(file_patterns) {
            let fp_stats = self.by_file_pattern.entry(pattern.clone()).or_default();
            update_pattern_dismissed(fp_stats);

            let composite = format!("{category}|{pattern}");
            let comp_stats = self.by_category_file_pattern.entry(composite).or_default();
            update_pattern_dismissed(comp_stats);
        }
    }

    /// Record an addressed/not-addressed outcome across one or more file-pattern buckets.
    pub fn record_outcome_patterns<S>(
        &mut self,
        category: &str,
        file_patterns: &[S],
        addressed: bool,
    ) where
        S: AsRef<str>,
    {
        let cat_stats = self.by_category.entry(category.to_string()).or_default();
        update_pattern_outcome(cat_stats, addressed);

        for pattern in collect_unique_patterns(file_patterns) {
            let fp_stats = self.by_file_pattern.entry(pattern.clone()).or_default();
            update_pattern_outcome(fp_stats, addressed);

            let composite = format!("{category}|{pattern}");
            let comp_stats = self.by_category_file_pattern.entry(composite).or_default();
            update_pattern_outcome(comp_stats, addressed);
        }
    }

    /// Record a dismissal event across normalized rule-id buckets.
    pub fn record_rule_dismissal_patterns<S>(&mut self, rule_id: &str, file_patterns: &[S])
    where
        S: AsRef<str>,
    {
        let Some(rule_id) = normalize_feedback_key(rule_id) else {
            return;
        };

        let rule_stats = self.by_rule.entry(rule_id.clone()).or_default();
        update_pattern_dismissed(rule_stats);

        for pattern in collect_unique_patterns(file_patterns) {
            let composite = format!("{}|{}", rule_id, pattern);
            let comp_stats = self.by_rule_file_pattern.entry(composite).or_default();
            update_pattern_dismissed(comp_stats);
        }
    }

    /// Record an addressed/not-addressed outcome across normalized rule-id buckets.
    pub fn record_rule_outcome_patterns<S>(
        &mut self,
        rule_id: &str,
        file_patterns: &[S],
        addressed: bool,
    ) where
        S: AsRef<str>,
    {
        self.record_rule_outcome_patterns_at(
            rule_id,
            file_patterns,
            addressed,
            current_timestamp(),
        );
    }

    /// Record an addressed/not-addressed outcome across normalized rule-id buckets at a specific timestamp.
    pub fn record_rule_outcome_patterns_at<S>(
        &mut self,
        rule_id: &str,
        file_patterns: &[S],
        addressed: bool,
        timestamp: i64,
    ) where
        S: AsRef<str>,
    {
        let Some(rule_id) = normalize_feedback_key(rule_id) else {
            return;
        };

        let rule_stats = self.by_rule.entry(rule_id.clone()).or_default();
        update_pattern_outcome(rule_stats, addressed);
        rule_stats.record_decayed_signal(addressed, timestamp);

        for pattern in collect_unique_patterns(file_patterns) {
            let composite = format!("{}|{}", rule_id, pattern);
            let comp_stats = self.by_rule_file_pattern.entry(composite).or_default();
            update_pattern_outcome(comp_stats, addressed);
            comp_stats.record_decayed_signal(addressed, timestamp);
        }
    }
}

fn current_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn update_pattern_stats(stats: &mut FeedbackPatternStats, accepted: bool) {
    if accepted {
        stats.accepted += 1;
    } else {
        stats.rejected += 1;
    }
}

fn update_pattern_dismissed(stats: &mut FeedbackPatternStats) {
    stats.dismissed += 1;
}

fn update_pattern_outcome(stats: &mut FeedbackPatternStats, addressed: bool) {
    if addressed {
        stats.addressed += 1;
    } else {
        stats.not_addressed += 1;
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
