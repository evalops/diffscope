use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A pattern extracted from PR comment history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRCommentPattern {
    pub pattern_text: String,
    pub frequency: usize,
    pub authors: Vec<String>,
    pub categories: Vec<String>,
    pub file_extensions: Vec<String>,
    pub avg_sentiment: f32,
}

impl PRCommentPattern {
    pub fn is_recurring(&self) -> bool {
        self.frequency >= 3
    }

    pub fn is_team_consensus(&self) -> bool {
        self.authors.len() >= 2 && self.frequency >= 3
    }

    pub fn relevance_for_file(&self, file_ext: &str) -> f32 {
        if self.file_extensions.is_empty() {
            return 0.5;
        }
        if self.file_extensions.iter().any(|e| e == file_ext) {
            1.0
        } else {
            0.1
        }
    }
}

/// A raw PR review comment for analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRReviewComment {
    pub body: String,
    pub author: String,
    pub file_path: Option<String>,
    pub created_at: String,
    pub state: Option<String>,
}

/// Analyzes PR comment history to learn team patterns.
#[derive(Debug, Default)]
pub struct PRHistoryAnalyzer {
    comments: Vec<PRReviewComment>,
    patterns: Vec<PRCommentPattern>,
    author_stats: HashMap<String, AuthorStats>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthorStats {
    pub comment_count: usize,
    pub top_categories: Vec<(String, usize)>,
    pub avg_comment_length: f32,
}

impl PRHistoryAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest a batch of PR review comments.
    pub fn ingest_comments(&mut self, comments: Vec<PRReviewComment>) {
        for comment in &comments {
            let stats = self
                .author_stats
                .entry(comment.author.clone())
                .or_default();
            stats.comment_count += 1;
            let n = stats.comment_count as f32;
            stats.avg_comment_length = stats.avg_comment_length * ((n - 1.0) / n)
                + comment.body.len() as f32 / n;
        }
        self.comments.extend(comments);
    }

    /// Extract recurring patterns from ingested comments.
    pub fn extract_patterns(&mut self) -> &[PRCommentPattern] {
        let mut token_map: HashMap<String, PatternAccumulator> = HashMap::new();

        for comment in &self.comments {
            let tokens = extract_review_tokens(&comment.body);
            let file_ext = comment
                .file_path
                .as_ref()
                .and_then(|p| {
                    std::path::Path::new(p)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            let category = classify_review_comment(&comment.body);
            let sentiment = estimate_sentiment(&comment.body);

            for token in &tokens {
                let acc = token_map.entry(token.clone()).or_default();
                acc.frequency += 1;
                if !acc.authors.contains(&comment.author) {
                    acc.authors.push(comment.author.clone());
                }
                if !category.is_empty() && !acc.categories.contains(&category) {
                    acc.categories.push(category.clone());
                }
                if !file_ext.is_empty() && !acc.file_extensions.contains(&file_ext) {
                    acc.file_extensions.push(file_ext.clone());
                }
                acc.sentiment_sum += sentiment;
                acc.sentiment_count += 1;
            }
        }

        self.patterns = token_map
            .into_iter()
            .filter(|(_, acc)| acc.frequency >= 2)
            .map(|(pattern, acc)| PRCommentPattern {
                pattern_text: pattern,
                frequency: acc.frequency,
                authors: acc.authors,
                categories: acc.categories,
                file_extensions: acc.file_extensions,
                avg_sentiment: if acc.sentiment_count > 0 {
                    acc.sentiment_sum / acc.sentiment_count as f32
                } else {
                    0.0
                },
            })
            .collect();

        self.patterns
            .sort_by(|a, b| b.frequency.cmp(&a.frequency));

        &self.patterns
    }

    /// Rank patterns by relevance to a specific file.
    pub fn rank_for_file(&self, file_ext: &str, max_results: usize) -> Vec<&PRCommentPattern> {
        let mut scored: Vec<(&PRCommentPattern, f32)> = self
            .patterns
            .iter()
            .map(|p| {
                let relevance = p.relevance_for_file(file_ext);
                let frequency_weight = (p.frequency as f32).ln().max(1.0);
                let consensus_weight = if p.is_team_consensus() { 1.5 } else { 1.0 };
                (p, relevance * frequency_weight * consensus_weight)
            })
            .collect();

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored
            .into_iter()
            .take(max_results)
            .map(|(p, _)| p)
            .collect()
    }

    /// Generate prompt guidance from PR history patterns.
    pub fn generate_review_guidance(&self, file_ext: &str) -> String {
        let top_patterns = self.rank_for_file(file_ext, 10);
        if top_patterns.is_empty() {
            return String::new();
        }

        let mut guidance = String::from("Based on team review history:\n");

        let recurring: Vec<_> = top_patterns
            .iter()
            .filter(|p| p.is_recurring())
            .take(5)
            .collect();
        if !recurring.is_empty() {
            guidance.push_str("\nRecurring review themes:\n");
            for p in recurring {
                guidance.push_str(&format!(
                    "- {} (seen {}x by {} reviewers)\n",
                    p.pattern_text,
                    p.frequency,
                    p.authors.len()
                ));
            }
        }

        let consensus: Vec<_> = top_patterns
            .iter()
            .filter(|p| p.is_team_consensus())
            .take(5)
            .collect();
        if !consensus.is_empty() {
            guidance.push_str("\nTeam consensus patterns:\n");
            for p in consensus {
                guidance.push_str(&format!(
                    "- {} ({}+ reviewers agree)\n",
                    p.pattern_text,
                    p.authors.len()
                ));
            }
        }

        guidance
    }

    /// Get statistics for a specific author.
    pub fn author_stats(&self, author: &str) -> Option<&AuthorStats> {
        self.author_stats.get(author)
    }

    pub fn comment_count(&self) -> usize {
        self.comments.len()
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

#[derive(Debug, Default)]
struct PatternAccumulator {
    frequency: usize,
    authors: Vec<String>,
    categories: Vec<String>,
    file_extensions: Vec<String>,
    sentiment_sum: f32,
    sentiment_count: usize,
}

/// Extract meaningful review tokens/phrases from a comment.
fn extract_review_tokens(body: &str) -> Vec<String> {
    let lower = body.to_lowercase();
    let mut tokens = Vec::new();

    // Extract key phrases using pattern matching
    let patterns = [
        "null check",
        "error handling",
        "edge case",
        "race condition",
        "memory leak",
        "sql injection",
        "type safety",
        "bounds check",
        "input validation",
        "dead code",
        "magic number",
        "naming convention",
        "code duplication",
        "test coverage",
        "performance impact",
        "security risk",
        "api design",
        "breaking change",
        "backwards compatible",
        "thread safety",
        "resource cleanup",
        "log level",
        "configuration",
        "documentation",
        "refactor",
    ];

    for pattern in &patterns {
        if lower.contains(pattern) {
            tokens.push(pattern.to_string());
        }
    }

    // Also extract significant words
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 4 && !is_common_word(w))
        .collect();

    for word in words.into_iter().take(5) {
        if !tokens.iter().any(|t| t.contains(word)) {
            tokens.push(word.to_string());
        }
    }

    tokens
}

fn is_common_word(word: &str) -> bool {
    matches!(
        word,
        "about" | "after" | "again" | "being" | "below" | "between"
        | "could" | "doing" | "during" | "every" | "first" | "found"
        | "going" | "great" | "having" | "here's" | "looks" | "maybe"
        | "might" | "other" | "please" | "right" | "seems" | "since"
        | "still" | "their" | "there" | "these" | "thing" | "think"
        | "those" | "under" | "until" | "using" | "where" | "which"
        | "while" | "would" | "should"
    )
}

/// Classify a PR review comment into a category.
fn classify_review_comment(body: &str) -> String {
    let lower = body.to_lowercase();

    if lower.contains("security") || lower.contains("vulnerability") || lower.contains("injection") {
        "security".to_string()
    } else if lower.contains("performance") || lower.contains("slow") || lower.contains("optimize") {
        "performance".to_string()
    } else if lower.contains("bug") || lower.contains("crash") || lower.contains("error") {
        "bug".to_string()
    } else if lower.contains("style") || lower.contains("naming") || lower.contains("format") {
        "style".to_string()
    } else if lower.contains("test") || lower.contains("coverage") {
        "testing".to_string()
    } else if lower.contains("doc") {
        "documentation".to_string()
    } else {
        "general".to_string()
    }
}

/// Estimate sentiment of a review comment (-1.0 to 1.0).
fn estimate_sentiment(body: &str) -> f32 {
    let lower = body.to_lowercase();
    let mut score: f32 = 0.0;

    let positive = ["good", "nice", "great", "excellent", "clean", "approve", "lgtm", "well done"];
    let negative = [
        "bug", "issue", "problem", "wrong", "broken", "fix", "missing",
        "incorrect", "fail", "error", "bad", "should not",
    ];

    for word in &positive {
        if lower.contains(word) {
            score += 0.2;
        }
    }
    for word in &negative {
        if lower.contains(word) {
            score -= 0.2;
        }
    }

    score.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_comment(body: &str, author: &str, file: Option<&str>) -> PRReviewComment {
        PRReviewComment {
            body: body.to_string(),
            author: author.to_string(),
            file_path: file.map(|f| f.to_string()),
            created_at: "2024-01-01".to_string(),
            state: None,
        }
    }

    #[test]
    fn test_ingest_and_extract_patterns() {
        let mut analyzer = PRHistoryAnalyzer::new();
        analyzer.ingest_comments(vec![
            make_comment("Missing null check here", "alice", Some("src/handler.rs")),
            make_comment("Null check needed for safety", "bob", Some("src/api.rs")),
            make_comment("Add null check for input", "alice", Some("src/parser.rs")),
        ]);

        let patterns = analyzer.extract_patterns();
        assert!(!patterns.is_empty());

        let null_check = patterns.iter().find(|p| p.pattern_text == "null check");
        assert!(null_check.is_some());
        assert_eq!(null_check.unwrap().frequency, 3);
    }

    #[test]
    fn test_recurring_pattern() {
        let p = PRCommentPattern {
            pattern_text: "error handling".to_string(),
            frequency: 5,
            authors: vec!["alice".to_string(), "bob".to_string()],
            categories: vec!["bug".to_string()],
            file_extensions: vec!["rs".to_string()],
            avg_sentiment: -0.2,
        };
        assert!(p.is_recurring());
        assert!(p.is_team_consensus());
    }

    #[test]
    fn test_not_recurring_pattern() {
        let p = PRCommentPattern {
            pattern_text: "minor style".to_string(),
            frequency: 1,
            authors: vec!["alice".to_string()],
            categories: vec![],
            file_extensions: vec![],
            avg_sentiment: 0.0,
        };
        assert!(!p.is_recurring());
        assert!(!p.is_team_consensus());
    }

    #[test]
    fn test_relevance_for_file() {
        let p = PRCommentPattern {
            pattern_text: "test".to_string(),
            frequency: 3,
            authors: vec![],
            categories: vec![],
            file_extensions: vec!["rs".to_string(), "py".to_string()],
            avg_sentiment: 0.0,
        };
        assert!((p.relevance_for_file("rs") - 1.0).abs() < 0.01);
        assert!((p.relevance_for_file("js") - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_rank_for_file() {
        let mut analyzer = PRHistoryAnalyzer::new();
        analyzer.ingest_comments(vec![
            make_comment("Null check needed", "alice", Some("src/foo.rs")),
            make_comment("Null check missing", "bob", Some("src/bar.rs")),
            make_comment("Null check required", "carol", Some("src/baz.rs")),
            make_comment("Edge case not handled", "alice", Some("src/foo.py")),
            make_comment("Edge case missing", "bob", Some("src/bar.py")),
            make_comment("Edge case bug", "carol", Some("src/baz.py")),
        ]);

        analyzer.extract_patterns();
        let ranked = analyzer.rank_for_file("rs", 5);
        assert!(!ranked.is_empty());
    }

    #[test]
    fn test_generate_review_guidance() {
        let mut analyzer = PRHistoryAnalyzer::new();
        for _ in 0..5 {
            analyzer.ingest_comments(vec![
                make_comment("Error handling is missing here", "alice", Some("src/api.rs")),
                make_comment("Error handling needed", "bob", Some("src/handler.rs")),
            ]);
        }
        analyzer.extract_patterns();

        let guidance = analyzer.generate_review_guidance("rs");
        assert!(guidance.contains("review history"));
    }

    #[test]
    fn test_author_stats() {
        let mut analyzer = PRHistoryAnalyzer::new();
        analyzer.ingest_comments(vec![
            make_comment("First comment", "alice", None),
            make_comment("Second longer comment", "alice", None),
            make_comment("Third comment", "bob", None),
        ]);

        let alice = analyzer.author_stats("alice").unwrap();
        assert_eq!(alice.comment_count, 2);
        assert!(alice.avg_comment_length > 0.0);

        let bob = analyzer.author_stats("bob").unwrap();
        assert_eq!(bob.comment_count, 1);
    }

    #[test]
    fn test_empty_analyzer() {
        let analyzer = PRHistoryAnalyzer::new();
        assert_eq!(analyzer.comment_count(), 0);
        assert_eq!(analyzer.pattern_count(), 0);
    }

    #[test]
    fn test_classify_review_comment() {
        assert_eq!(classify_review_comment("This is a security vulnerability"), "security");
        assert_eq!(classify_review_comment("Performance is slow here"), "performance");
        assert_eq!(classify_review_comment("This could crash the server"), "bug");
        assert_eq!(classify_review_comment("Naming convention mismatch"), "style");
    }

    #[test]
    fn test_estimate_sentiment() {
        assert!(estimate_sentiment("Great work, looks good!") > 0.0);
        assert!(estimate_sentiment("This is wrong and broken") < 0.0);
        assert!(estimate_sentiment("Changed line 5") > -0.5); // neutral-ish
    }

    #[test]
    fn test_guidance_empty_for_no_comments() {
        let analyzer = PRHistoryAnalyzer::new();
        let guidance = analyzer.generate_review_guidance("rs");
        assert!(guidance.is_empty());
    }

    #[test]
    fn test_single_comment_no_recurring_patterns() {
        let mut analyzer = PRHistoryAnalyzer::new();
        analyzer.ingest_comments(vec![PRReviewComment {
            body: "Fix this bug".to_string(),
            author: "alice".to_string(),
            file_path: Some("src/main.rs".to_string()),
            created_at: "2024-01-01".to_string(),
            state: None,
        }]);
        // Single comment shouldn't create recurring patterns
        let patterns = analyzer.extract_patterns();
        for p in patterns {
            assert!(!p.is_recurring(), "Single comment shouldn't be recurring");
        }
    }

    #[test]
    fn test_relevance_for_unknown_extension() {
        let pattern = PRCommentPattern {
            pattern_text: "test".to_string(),
            frequency: 5,
            authors: vec!["alice".to_string()],
            categories: vec!["bug".to_string()],
            file_extensions: vec!["rs".to_string()],
            avg_sentiment: 0.0,
        };
        // Non-matching extension should get low relevance
        assert!(pattern.relevance_for_file("py") < 0.5);
        // Matching extension should get high relevance
        assert!(pattern.relevance_for_file("rs") > 0.5);
    }

    #[test]
    fn test_team_consensus_requires_multiple_authors() {
        let single_author = PRCommentPattern {
            pattern_text: "test".to_string(),
            frequency: 5,
            authors: vec!["alice".to_string()],
            categories: vec![],
            file_extensions: vec![],
            avg_sentiment: 0.0,
        };
        assert!(!single_author.is_team_consensus());

        let multi_author = PRCommentPattern {
            pattern_text: "test".to_string(),
            frequency: 5,
            authors: vec!["alice".to_string(), "bob".to_string()],
            categories: vec![],
            file_extensions: vec![],
            avg_sentiment: 0.0,
        };
        assert!(multi_author.is_team_consensus());
    }

    #[test]
    fn test_empty_body_comments() {
        let mut analyzer = PRHistoryAnalyzer::new();
        analyzer.ingest_comments(vec![PRReviewComment {
            body: "".to_string(),
            author: "alice".to_string(),
            file_path: None,
            created_at: "2024-01-01".to_string(),
            state: None,
        }]);
        // Should handle empty bodies gracefully
        assert_eq!(analyzer.comment_count(), 1);
    }

    // BUG: extract_patterns replaces all patterns on every call, losing prior data
    #[test]
    fn test_extract_patterns_preserves_across_ingests() {
        let mut analyzer = PRHistoryAnalyzer::new();

        // Ingest 3 comments with the same token so it hits frequency >= 2
        analyzer.ingest_comments(vec![
            PRReviewComment {
                body: "Missing error handling here".to_string(),
                author: "alice".to_string(),
                file_path: Some("src/main.rs".to_string()),
                created_at: "2024-01-01".to_string(),
                state: None,
            },
            PRReviewComment {
                body: "Missing error handling again".to_string(),
                author: "bob".to_string(),
                file_path: Some("src/lib.rs".to_string()),
                created_at: "2024-01-02".to_string(),
                state: None,
            },
        ]);

        let patterns_first = analyzer.extract_patterns();
        let first_count = patterns_first.len();
        assert!(first_count > 0, "Should have patterns from first batch");

        // Ingest more comments with a different pattern
        analyzer.ingest_comments(vec![
            PRReviewComment {
                body: "Security vulnerability found".to_string(),
                author: "carol".to_string(),
                file_path: Some("src/auth.rs".to_string()),
                created_at: "2024-01-03".to_string(),
                state: None,
            },
            PRReviewComment {
                body: "Security vulnerability detected".to_string(),
                author: "dave".to_string(),
                file_path: Some("src/auth.rs".to_string()),
                created_at: "2024-01-04".to_string(),
                state: None,
            },
        ]);

        let patterns_second = analyzer.extract_patterns();
        // Should have patterns from BOTH batches
        assert!(
            patterns_second.len() >= first_count,
            "Second extract should have >= patterns (had {}, now {})",
            first_count,
            patterns_second.len()
        );
    }
}
