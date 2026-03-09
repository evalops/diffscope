use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A learned pattern from review feedback (accepted/rejected comments).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionPattern {
    pub pattern_text: String,
    pub category: String,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub file_patterns: Vec<String>,
    pub first_seen: String,
    pub last_seen: String,
}

impl ConventionPattern {
    pub fn acceptance_rate(&self) -> f32 {
        let total = self.accepted_count + self.rejected_count;
        if total == 0 {
            return 0.0;
        }
        self.accepted_count as f32 / total as f32
    }

    pub fn total_observations(&self) -> usize {
        self.accepted_count + self.rejected_count
    }

    pub fn confidence(&self) -> f32 {
        let n = self.total_observations() as f32;
        if n < 2.0 {
            return 0.0;
        }
        // Wilson score interval lower bound for confidence
        let p = self.acceptance_rate();
        let z = 1.96; // 95% confidence
        let denominator = 1.0 + z * z / n;
        let center = p + z * z / (2.0 * n);
        let spread = z * ((p * (1.0 - p) / n) + (z * z / (4.0 * n * n))).sqrt();
        ((center - spread) / denominator).clamp(0.0, 1.0)
    }

    /// Whether this pattern should suppress future similar findings.
    pub fn should_suppress(&self) -> bool {
        self.total_observations() >= 3 && self.acceptance_rate() < 0.25
    }

    /// Whether this pattern should boost confidence of future findings.
    pub fn should_boost(&self) -> bool {
        self.total_observations() >= 3 && self.acceptance_rate() > 0.75
    }
}

/// Persistent store of learned conventions from review feedback.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConventionStore {
    patterns: HashMap<String, ConventionPattern>,
    /// Token frequency across accepted comments for weighting.
    accepted_tokens: HashMap<String, usize>,
    /// Token frequency across rejected comments.
    rejected_tokens: HashMap<String, usize>,
    version: u32,
}

impl ConventionStore {
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
            accepted_tokens: HashMap::new(),
            rejected_tokens: HashMap::new(),
            version: 1,
        }
    }

    /// Record a user's accept/reject feedback on a review comment.
    pub fn record_feedback(
        &mut self,
        comment_content: &str,
        category: &str,
        accepted: bool,
        file_pattern: Option<&str>,
        timestamp: &str,
    ) {
        let key = normalize_pattern(comment_content);
        if key.is_empty() {
            return;
        }

        let pattern = self
            .patterns
            .entry(key.clone())
            .or_insert_with(|| ConventionPattern {
                pattern_text: key,
                category: category.to_string(),
                accepted_count: 0,
                rejected_count: 0,
                file_patterns: Vec::new(),
                first_seen: timestamp.to_string(),
                last_seen: timestamp.to_string(),
            });

        if accepted {
            pattern.accepted_count += 1;
        } else {
            pattern.rejected_count += 1;
        }
        pattern.last_seen = timestamp.to_string();

        if let Some(fp) = file_pattern {
            if !pattern.file_patterns.contains(&fp.to_string()) {
                pattern.file_patterns.push(fp.to_string());
            }
        }

        // Update token frequencies
        let tokens = extract_tokens(comment_content);
        let token_map = if accepted {
            &mut self.accepted_tokens
        } else {
            &mut self.rejected_tokens
        };
        for token in tokens {
            *token_map.entry(token).or_insert(0) += 1;
        }
    }

    /// Get patterns that match the given category and file extension.
    pub fn matching_patterns(
        &self,
        category: &str,
        file_ext: Option<&str>,
    ) -> Vec<&ConventionPattern> {
        self.patterns
            .values()
            .filter(|p| {
                p.category == category
                    && p.total_observations() >= 2
                    && (file_ext.is_none()
                        || p.file_patterns.is_empty()
                        || p.file_patterns
                            .iter()
                            .any(|fp| file_ext.is_some_and(|e| fp.contains(e))))
            })
            .collect()
    }

    /// Get patterns that should suppress future findings.
    pub fn suppression_patterns(&self) -> Vec<&ConventionPattern> {
        self.patterns
            .values()
            .filter(|p| p.should_suppress())
            .collect()
    }

    /// Get patterns that should boost confidence.
    pub fn boost_patterns(&self) -> Vec<&ConventionPattern> {
        self.patterns
            .values()
            .filter(|p| p.should_boost())
            .collect()
    }

    /// Score a new comment based on learned conventions.
    /// Returns a confidence adjustment (-1.0 to +1.0).
    pub fn score_comment(&self, content: &str, category: &str) -> f32 {
        let tokens = extract_tokens(content);
        if tokens.is_empty() {
            return 0.0;
        }

        let mut accepted_weight: f32 = 0.0;
        let mut rejected_weight: f32 = 0.0;

        for token in &tokens {
            if let Some(&count) = self.accepted_tokens.get(token) {
                accepted_weight += count as f32;
            }
            if let Some(&count) = self.rejected_tokens.get(token) {
                rejected_weight += count as f32;
            }
        }

        let total = accepted_weight + rejected_weight;
        if total < 3.0 {
            return 0.0;
        }

        // Pattern matching boost/penalty
        let pattern_key = normalize_pattern(content);
        if let Some(pattern) = self.patterns.get(&pattern_key) {
            if pattern.category == category {
                if pattern.should_suppress() {
                    return -0.3;
                }
                if pattern.should_boost() {
                    return 0.2;
                }
            }
        }

        // Token-based scoring
        let ratio = accepted_weight / total;
        (ratio - 0.5).clamp(-0.3, 0.3)
    }

    /// Generate convention summary for prompt injection.
    pub fn generate_guidance(&self, categories: &[&str]) -> String {
        let mut guidance = String::new();

        for category in categories {
            let boost: Vec<_> = self
                .matching_patterns(category, None)
                .into_iter()
                .filter(|p| p.should_boost())
                .collect();
            let suppress: Vec<_> = self
                .matching_patterns(category, None)
                .into_iter()
                .filter(|p| p.should_suppress())
                .collect();

            if !boost.is_empty() {
                guidance.push_str(&format!(
                    "\nHigh-value {} patterns (team accepts these):\n",
                    category
                ));
                for p in boost.iter().take(5) {
                    guidance.push_str(&format!(
                        "- {} (accepted {}x)\n",
                        p.pattern_text, p.accepted_count
                    ));
                }
            }

            if !suppress.is_empty() {
                guidance.push_str(&format!(
                    "\nLow-value {} patterns (team rejects these):\n",
                    category
                ));
                for p in suppress.iter().take(5) {
                    guidance.push_str(&format!(
                        "- {} (rejected {}x)\n",
                        p.pattern_text, p.rejected_count
                    ));
                }
            }
        }

        guidance
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Normalize comment text into a pattern key (lowercased, stopwords removed).
fn normalize_pattern(text: &str) -> String {
    let lower = text.to_lowercase();
    let tokens: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(w))
        .map(|w| w.to_string())
        .collect();
    tokens.join(" ")
}

/// Extract meaningful tokens from text.
fn extract_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "has", "her", "was", "one",
    "our", "out", "its", "his", "how", "man", "new", "now", "old", "see", "way", "who", "did",
    "get", "let", "say", "she", "too", "use", "this", "that", "with", "have", "from", "they",
    "been", "will", "more", "when", "some", "them", "than", "here", "into", "should", "could",
    "would", "which", "there", "their", "about",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_retrieve_feedback() {
        let mut store = ConventionStore::new();
        store.record_feedback(
            "Missing error handling on API call",
            "BestPractice",
            true,
            Some("*.rs"),
            "2024-01-01",
        );
        store.record_feedback(
            "Missing error handling on API call",
            "BestPractice",
            true,
            None,
            "2024-01-02",
        );

        assert_eq!(store.pattern_count(), 1);
        let patterns = store.matching_patterns("BestPractice", None);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].accepted_count, 2);
    }

    #[test]
    fn test_acceptance_rate() {
        let p = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Style".to_string(),
            accepted_count: 3,
            rejected_count: 1,
            file_patterns: vec![],
            first_seen: "".to_string(),
            last_seen: "".to_string(),
        };
        assert!((p.acceptance_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_should_suppress() {
        let p = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Style".to_string(),
            accepted_count: 0,
            rejected_count: 5,
            file_patterns: vec![],
            first_seen: "".to_string(),
            last_seen: "".to_string(),
        };
        assert!(p.should_suppress());
        assert!(!p.should_boost());
    }

    #[test]
    fn test_should_boost() {
        let p = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 8,
            rejected_count: 1,
            file_patterns: vec![],
            first_seen: "".to_string(),
            last_seen: "".to_string(),
        };
        assert!(p.should_boost());
        assert!(!p.should_suppress());
    }

    #[test]
    fn test_confidence_increases_with_observations() {
        let p1 = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 2,
            rejected_count: 0,
            file_patterns: vec![],
            first_seen: "".to_string(),
            last_seen: "".to_string(),
        };
        let p2 = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 20,
            rejected_count: 0,
            file_patterns: vec![],
            first_seen: "".to_string(),
            last_seen: "".to_string(),
        };
        assert!(p2.confidence() > p1.confidence());
    }

    #[test]
    fn test_score_comment_no_history() {
        let store = ConventionStore::new();
        let score = store.score_comment("Brand new comment", "Bug");
        assert!((score - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_score_comment_with_positive_history() {
        let mut store = ConventionStore::new();
        for _ in 0..10 {
            store.record_feedback(
                "Missing null check on user input",
                "Bug",
                true,
                None,
                "2024-01-01",
            );
        }

        let score = store.score_comment("null check missing for user input", "Bug");
        assert!(score > 0.0, "Expected positive score, got {}", score);
    }

    #[test]
    fn test_score_comment_with_negative_history() {
        let mut store = ConventionStore::new();
        for _ in 0..10 {
            store.record_feedback(
                "Consider adding more comments",
                "Style",
                false,
                None,
                "2024-01-01",
            );
        }
        // Exact same pattern
        store.record_feedback(
            "Consider adding more comments",
            "Style",
            false,
            None,
            "2024-01-01",
        );

        let score = store.score_comment("Consider adding more comments", "Style");
        assert!(score < 0.0, "Expected negative score, got {}", score);
    }

    #[test]
    fn test_generate_guidance() {
        let mut store = ConventionStore::new();
        for _ in 0..5 {
            store.record_feedback(
                "SQL injection risk via string concatenation",
                "Security",
                true,
                None,
                "2024-01-01",
            );
        }
        for _ in 0..5 {
            store.record_feedback(
                "Missing trailing comma in imports",
                "Style",
                false,
                None,
                "2024-01-01",
            );
        }

        let guidance = store.generate_guidance(&["Security", "Style"]);
        assert!(guidance.contains("High-value Security"));
        assert!(guidance.contains("Low-value Style"));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut store = ConventionStore::new();
        store.record_feedback("test pattern", "Bug", true, Some("*.rs"), "2024-01-01");

        let json = store.to_json().unwrap();
        let restored = ConventionStore::from_json(&json).unwrap();
        assert_eq!(restored.pattern_count(), 1);
    }

    #[test]
    fn test_matching_patterns_by_file_ext() {
        let mut store = ConventionStore::new();
        store.record_feedback("test pattern", "Bug", true, Some("*.rs"), "2024-01-01");
        store.record_feedback("test pattern", "Bug", true, Some("*.rs"), "2024-01-02");

        let rust = store.matching_patterns("Bug", Some("rs"));
        assert_eq!(rust.len(), 1);

        let python = store.matching_patterns("Bug", Some("py"));
        assert_eq!(python.len(), 0);
    }

    #[test]
    fn test_empty_feedback_ignored() {
        let mut store = ConventionStore::new();
        store.record_feedback("", "Bug", true, None, "2024-01-01");
        assert_eq!(store.pattern_count(), 0);
    }

    #[test]
    fn test_file_pattern_deduplication() {
        let mut store = ConventionStore::new();
        store.record_feedback("test", "Bug", true, Some("*.rs"), "2024-01-01");
        store.record_feedback("test", "Bug", true, Some("*.rs"), "2024-01-02");
        store.record_feedback("test", "Bug", true, Some("*.py"), "2024-01-03");

        let patterns = store.matching_patterns("Bug", None);
        assert_eq!(patterns[0].file_patterns.len(), 2);
    }

    #[test]
    fn test_normalize_pattern_removes_stopwords() {
        let result = normalize_pattern("the missing error handling for this API call");
        assert!(!result.contains("the"));
        assert!(!result.contains("for"));
        assert!(result.contains("missing"));
        assert!(result.contains("error"));
    }

    #[test]
    fn test_boost_patterns() {
        let mut store = ConventionStore::new();
        for _ in 0..5 {
            store.record_feedback("important bug pattern", "Bug", true, None, "2024-01-01");
        }
        let boosted = store.boost_patterns();
        assert_eq!(boosted.len(), 1);
        assert!(boosted[0].should_boost());
    }

    #[test]
    fn test_suppression_patterns() {
        let mut store = ConventionStore::new();
        for _ in 0..5 {
            store.record_feedback("noisy pattern", "Style", false, None, "2024-01-01");
        }
        let suppressed = store.suppression_patterns();
        assert_eq!(suppressed.len(), 1);
    }

    #[test]
    fn test_normalize_all_stopwords_returns_empty() {
        let result = normalize_pattern("the and for are but not");
        assert!(result.is_empty());
    }

    #[test]
    fn test_normalize_short_words_filtered() {
        let result = normalize_pattern("a b c do be");
        assert!(result.is_empty());
    }

    #[test]
    fn test_normalize_strips_punctuation() {
        let a = normalize_pattern("Missing error-handling for API calls");
        let b = normalize_pattern("Missing error handling for API calls");
        // Hyphen should be treated as separator, matching split behavior of extract_tokens
        assert!(a.contains("missing"));
        assert!(a.contains("error"));
        assert!(a.contains("handling"));
        assert_eq!(a, b);
    }

    #[test]
    fn test_confidence_single_observation() {
        let pattern = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 1,
            rejected_count: 0,
            file_patterns: Vec::new(),
            first_seen: "2024-01-01".to_string(),
            last_seen: "2024-01-01".to_string(),
        };
        // Single observation should return 0 confidence (not enough data)
        assert_eq!(pattern.confidence(), 0.0);
    }

    #[test]
    fn test_confidence_all_accepted() {
        let pattern = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 10,
            rejected_count: 0,
            file_patterns: Vec::new(),
            first_seen: "2024-01-01".to_string(),
            last_seen: "2024-01-01".to_string(),
        };
        let conf = pattern.confidence();
        assert!(
            conf > 0.5,
            "High acceptance should yield high confidence: {conf}"
        );
    }

    #[test]
    fn test_confidence_all_rejected() {
        let pattern = ConventionPattern {
            pattern_text: "test".to_string(),
            category: "Bug".to_string(),
            accepted_count: 0,
            rejected_count: 10,
            file_patterns: Vec::new(),
            first_seen: "2024-01-01".to_string(),
            last_seen: "2024-01-01".to_string(),
        };
        let conf = pattern.confidence();
        assert!(
            conf < 0.1,
            "All rejected should yield low confidence: {conf}"
        );
    }

    #[test]
    fn test_score_comment_pattern_fallthrough() {
        // Pattern exists, category matches, but neither suppress nor boost threshold met
        let mut store = ConventionStore::new();
        // 2 accepted, 1 rejected = 66% acceptance (below boost 75%, above suppress 25%)
        store.record_feedback("borderline pattern", "Bug", true, None, "2024-01-01");
        store.record_feedback("borderline pattern", "Bug", true, None, "2024-01-02");
        store.record_feedback("borderline pattern", "Bug", false, None, "2024-01-03");

        let score = store.score_comment("borderline pattern", "Bug");
        // Should fall through to token-based scoring, not hit suppress/boost early returns
        assert!(score.abs() <= 0.3);
    }

    #[test]
    fn test_record_feedback_empty_comment() {
        let mut store = ConventionStore::new();
        store.record_feedback("", "Bug", true, None, "2024-01-01");
        // Empty comment normalizes to empty key, should be rejected
        assert!(store.patterns.is_empty());
    }

    #[test]
    fn test_extract_tokens_consistency() {
        // Verify extract_tokens and normalize_pattern produce compatible results
        let text = "Missing error handling for API call";
        let normalized = normalize_pattern(text);
        let tokens = extract_tokens(text);
        // Every token should appear in the normalized pattern
        for token in &tokens {
            assert!(
                normalized.contains(token.as_str()),
                "Token '{token}' not found in normalized '{normalized}'"
            );
        }
    }

    // Test: pattern boost should only apply when category matches
    #[test]
    fn test_score_comment_wrong_category_skips_pattern_boost() {
        let mut store = ConventionStore::new();
        // Record lots of accepted feedback — pattern should boost for "Bug"
        for _ in 0..5 {
            store.record_feedback("important security check", "Bug", true, None, "2024-01-01");
        }
        // Score with matching category — should get boost
        let bug_score = store.score_comment("important security check", "Bug");
        assert!(
            bug_score > 0.0,
            "Bug category should get boost, got {bug_score}"
        );

        // Score with wrong category — should NOT get pattern boost of 0.2
        // Falls through to token-based scoring instead
        let style_score = store.score_comment("important security check", "Style");
        // Token-based: all tokens are accepted → ratio near 1.0 → (1.0 - 0.5) = 0.5 → clamped to 0.3
        assert!(
            style_score <= 0.3,
            "Wrong category should get at most token-based score, got {style_score}"
        );
        // The pattern boost for Bug (0.2) should NOT equal the token score
        assert_ne!(
            style_score, bug_score,
            "Different categories should score differently"
        );
    }

    // Regression: record_feedback must count both feedbacks even with different categories
    #[test]
    fn test_record_feedback_category_override() {
        let mut store = ConventionStore::new();
        store.record_feedback("Missing null check", "Bug", true, None, "2024-01-01");
        store.record_feedback("Missing null check", "Style", true, None, "2024-01-02");

        // The pattern key is the same, but category was set on first insert
        let key = normalize_pattern("Missing null check");
        let pattern = store.patterns.get(&key).unwrap();
        // Category should reflect the first (or most common) category — currently it's stuck
        assert_eq!(pattern.accepted_count, 2, "Both feedbacks should count");
    }
}
