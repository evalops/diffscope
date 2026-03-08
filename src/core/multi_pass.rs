use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::comment::Comment;
use crate::core::diff_parser::{ChangeType, UnifiedDiff};

/// Configuration for multi-pass review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPassConfig {
    pub enable_hotspot_pass: bool,
    pub enable_deep_pass: bool,
    pub hotspot_threshold: f32,
    pub max_deep_files: usize,
    pub deep_context_multiplier: usize,
}

impl Default for MultiPassConfig {
    fn default() -> Self {
        Self {
            enable_hotspot_pass: true,
            enable_deep_pass: true,
            hotspot_threshold: 0.5,
            max_deep_files: 5,
            deep_context_multiplier: 3,
        }
    }
}

/// Result of the hotspot detection pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotResult {
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub risk_score: f32,
    pub reasons: Vec<String>,
    pub suggested_focus: Vec<String>,
}

impl HotspotResult {
    pub fn is_high_risk(&self, threshold: f32) -> bool {
        self.risk_score >= threshold
    }
}

/// Orchestrates multi-pass review: hotspot detection then deep analysis.
#[derive(Debug)]
pub struct MultiPassReview {
    config: MultiPassConfig,
}

impl MultiPassReview {
    pub fn new(config: MultiPassConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(MultiPassConfig::default())
    }

    /// Pass 1: Identify hotspots in diffs using heuristic risk analysis.
    pub fn detect_hotspots(&self, diffs: &[UnifiedDiff]) -> Vec<HotspotResult> {
        let mut hotspots = Vec::new();

        for diff in diffs {
            if diff.is_binary || diff.hunks.is_empty() {
                continue;
            }

            let file_risk = analyze_file_risk(diff);
            if file_risk.risk_score > 0.0 {
                hotspots.push(file_risk);
            }
        }

        hotspots.sort_by(|a, b| {
            b.risk_score
                .partial_cmp(&a.risk_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        hotspots
    }

    /// Filter hotspots above threshold for deep analysis.
    pub fn select_for_deep_analysis<'a>(&self, hotspots: &'a [HotspotResult]) -> Vec<&'a HotspotResult> {
        hotspots
            .iter()
            .filter(|h| h.is_high_risk(self.config.hotspot_threshold))
            .take(self.config.max_deep_files)
            .collect()
    }

    /// Build enhanced prompts for deep analysis pass based on hotspot results.
    pub fn build_deep_analysis_guidance(
        &self,
        hotspot: &HotspotResult,
    ) -> String {
        let mut guidance = String::new();

        guidance.push_str(&format!(
            "DEEP ANALYSIS MODE - Risk score: {:.2}/1.0\n",
            hotspot.risk_score
        ));
        guidance.push_str("Focus areas:\n");

        for (i, focus) in hotspot.suggested_focus.iter().enumerate() {
            guidance.push_str(&format!("{}. {}\n", i + 1, focus));
        }

        guidance.push_str("\nRisk factors identified:\n");
        for reason in &hotspot.reasons {
            guidance.push_str(&format!("- {}\n", reason));
        }

        guidance.push_str(&format!(
            "\nAnalyze lines {}-{} with extra scrutiny.\n",
            hotspot.line_range.0, hotspot.line_range.1
        ));

        guidance
    }

    /// Pass 2: Merge deep analysis results with first-pass results.
    pub fn merge_results(
        &self,
        first_pass: Vec<Comment>,
        deep_pass: Vec<Comment>,
    ) -> Vec<Comment> {
        let mut merged = first_pass;

        for deep_comment in deep_pass {
            // Check for duplicate by location + content similarity
            let is_duplicate = merged.iter().any(|existing| {
                existing.file_path == deep_comment.file_path
                    && existing.line_number == deep_comment.line_number
                    && content_similarity(&existing.content, &deep_comment.content) > 0.7
            });

            if !is_duplicate {
                // Boost confidence for deep-pass findings
                let mut boosted = deep_comment;
                boosted.confidence = (boosted.confidence * 1.15).min(1.0);
                boosted.tags.push("deep-analysis".to_string());
                merged.push(boosted);
            }
        }

        merged
    }

    /// Generate a summary of the multi-pass review.
    pub fn summarize_passes(
        &self,
        hotspots: &[HotspotResult],
        first_pass_count: usize,
        deep_pass_count: usize,
        merged_count: usize,
    ) -> MultiPassSummary {
        let high_risk_files = hotspots
            .iter()
            .filter(|h| h.is_high_risk(self.config.hotspot_threshold))
            .count();

        MultiPassSummary {
            total_files_scanned: hotspots.len(),
            high_risk_files,
            first_pass_findings: first_pass_count,
            deep_pass_findings: deep_pass_count,
            merged_findings: merged_count,
            hotspot_threshold: self.config.hotspot_threshold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPassSummary {
    pub total_files_scanned: usize,
    pub high_risk_files: usize,
    pub first_pass_findings: usize,
    pub deep_pass_findings: usize,
    pub merged_findings: usize,
    pub hotspot_threshold: f32,
}

/// Analyze a single diff for risk factors.
fn analyze_file_risk(diff: &UnifiedDiff) -> HotspotResult {
    let mut risk_score: f32 = 0.0;
    let mut reasons = Vec::new();
    let mut suggested_focus = Vec::new();

    let file_str = diff.file_path.to_string_lossy().to_lowercase();

    // File path risk factors
    if file_str.contains("auth") || file_str.contains("security") || file_str.contains("crypto") {
        risk_score += 0.3;
        reasons.push("Security-sensitive file path".to_string());
        suggested_focus.push("Check for authentication/authorization vulnerabilities".to_string());
    }
    if file_str.contains("payment") || file_str.contains("billing") || file_str.contains("money") {
        risk_score += 0.25;
        reasons.push("Financial/payment-related code".to_string());
        suggested_focus.push("Verify transaction integrity and error handling".to_string());
    }
    if file_str.contains("migration") || file_str.contains("schema") {
        risk_score += 0.2;
        reasons.push("Database schema change".to_string());
        suggested_focus.push("Check for data loss risks and backwards compatibility".to_string());
    }

    // Change volume risk
    let total_added: usize = diff
        .hunks
        .iter()
        .flat_map(|h| &h.changes)
        .filter(|c| c.change_type == ChangeType::Added)
        .count();
    let total_removed: usize = diff
        .hunks
        .iter()
        .flat_map(|h| &h.changes)
        .filter(|c| c.change_type == ChangeType::Removed)
        .count();
    let total_changes = total_added + total_removed;

    if total_changes > 100 {
        risk_score += 0.2;
        reasons.push(format!("Large change volume ({} lines)", total_changes));
    } else if total_changes > 50 {
        risk_score += 0.1;
        reasons.push(format!("Moderate change volume ({} lines)", total_changes));
    }

    // Content risk patterns
    let all_content: String = diff
        .hunks
        .iter()
        .flat_map(|h| &h.changes)
        .filter(|c| c.change_type == ChangeType::Added)
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let lower_content = all_content.to_lowercase();

    let risk_patterns = [
        ("unsafe", 0.25, "Uses unsafe code"),
        ("unwrap()", 0.1, "Uses unwrap() without error handling"),
        ("exec(", 0.2, "Dynamic code execution"),
        ("eval(", 0.25, "Code evaluation"),
        ("todo!", 0.05, "Contains TODO markers"),
        ("fixme", 0.05, "Contains FIXME markers"),
        ("password", 0.15, "References passwords/credentials"),
        ("secret", 0.15, "References secrets"),
        ("sql", 0.15, "Contains SQL operations"),
        ("deserialize", 0.1, "Deserialization (potential RCE vector)"),
        ("system(", 0.2, "System command execution"),
        ("shell", 0.15, "Shell command interaction"),
        ("chmod", 0.1, "File permission changes"),
        ("panic!", 0.1, "Contains explicit panic"),
    ];

    for (pattern, weight, reason) in &risk_patterns {
        if lower_content.contains(pattern) {
            risk_score += weight;
            reasons.push(reason.to_string());
        }
    }

    // Deleted code without replacement (potential regression)
    if total_removed > total_added * 2 && total_removed > 10 {
        risk_score += 0.15;
        reasons.push("Significant code deletion without replacement".to_string());
        suggested_focus.push("Verify no functionality regression from deleted code".to_string());
    }

    // New file risk (less review context)
    if diff.is_new && total_added > 50 {
        risk_score += 0.1;
        reasons.push("Large new file with no review history".to_string());
    }

    // Compute line range
    let min_line = diff
        .hunks
        .iter()
        .map(|h| h.new_start)
        .min()
        .unwrap_or(1);
    let max_line = diff
        .hunks
        .iter()
        .map(|h| h.new_start + h.new_lines)
        .max()
        .unwrap_or(1);

    risk_score = risk_score.clamp(0.0, 1.0);

    if suggested_focus.is_empty() && risk_score > 0.3 {
        suggested_focus.push("Review for correctness and edge cases".to_string());
    }

    HotspotResult {
        file_path: diff.file_path.clone(),
        line_range: (min_line, max_line),
        risk_score,
        reasons,
        suggested_focus,
    }
}

/// Simple content similarity based on shared words.
fn content_similarity(a: &str, b: &str) -> f32 {
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }
    if words_a.is_empty() || words_b.is_empty() {
        return 0.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use crate::core::diff_parser::{DiffHunk, DiffLine};

    fn make_diff(path: &str, changes: Vec<DiffLine>) -> UnifiedDiff {
        UnifiedDiff {
            file_path: PathBuf::from(path),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: changes.len(),
                new_start: 1,
                new_lines: changes.len(),
                context: "@@ -1 +1 @@".to_string(),
                changes,
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    fn make_added_line(line: usize, content: &str) -> DiffLine {
        DiffLine {
            old_line_no: None,
            new_line_no: Some(line),
            change_type: ChangeType::Added,
            content: content.to_string(),
        }
    }

    fn make_comment(file: &str, line: usize, content: &str) -> Comment {
        Comment {
            id: format!("cmt_{}", line),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::BestPractice,
            suggestion: None,
            confidence: 0.7,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Medium,
        }
    }

    #[test]
    fn test_detect_hotspots_security_file() {
        let review = MultiPassReview::with_defaults();
        let diffs = vec![make_diff(
            "src/auth/login.rs",
            vec![make_added_line(1, "let password = get_input();")],
        )];

        let hotspots = review.detect_hotspots(&diffs);
        assert_eq!(hotspots.len(), 1);
        assert!(hotspots[0].risk_score > 0.3);
        assert!(hotspots[0]
            .reasons
            .iter()
            .any(|r| r.contains("Security")));
    }

    #[test]
    fn test_detect_hotspots_unsafe_code() {
        let review = MultiPassReview::with_defaults();
        let diffs = vec![make_diff(
            "src/lib.rs",
            vec![make_added_line(1, "unsafe { ptr::read(addr) }")],
        )];

        let hotspots = review.detect_hotspots(&diffs);
        assert!(!hotspots.is_empty());
        assert!(hotspots[0]
            .reasons
            .iter()
            .any(|r| r.contains("unsafe")));
    }

    #[test]
    fn test_detect_hotspots_large_change() {
        let review = MultiPassReview::with_defaults();
        let changes: Vec<DiffLine> = (0..120)
            .map(|i| make_added_line(i + 1, "some code line"))
            .collect();
        let diffs = vec![make_diff("src/big.rs", changes)];

        let hotspots = review.detect_hotspots(&diffs);
        assert!(!hotspots.is_empty());
        assert!(hotspots[0]
            .reasons
            .iter()
            .any(|r| r.contains("Large change")));
    }

    #[test]
    fn test_detect_hotspots_skips_binary() {
        let review = MultiPassReview::with_defaults();
        let diffs = vec![UnifiedDiff {
            file_path: PathBuf::from("image.png"),
            old_content: None,
            new_content: None,
            hunks: vec![],
            is_binary: true,
            is_deleted: false,
            is_new: false,
        }];

        let hotspots = review.detect_hotspots(&diffs);
        assert!(hotspots.is_empty());
    }

    #[test]
    fn test_select_for_deep_analysis() {
        let review = MultiPassReview::new(MultiPassConfig {
            hotspot_threshold: 0.3,
            max_deep_files: 2,
            ..Default::default()
        });

        let hotspots = vec![
            HotspotResult {
                file_path: PathBuf::from("a.rs"),
                line_range: (1, 10),
                risk_score: 0.8,
                reasons: vec![],
                suggested_focus: vec![],
            },
            HotspotResult {
                file_path: PathBuf::from("b.rs"),
                line_range: (1, 10),
                risk_score: 0.5,
                reasons: vec![],
                suggested_focus: vec![],
            },
            HotspotResult {
                file_path: PathBuf::from("c.rs"),
                line_range: (1, 10),
                risk_score: 0.1,
                reasons: vec![],
                suggested_focus: vec![],
            },
        ];

        let selected = review.select_for_deep_analysis(&hotspots);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].file_path, PathBuf::from("a.rs"));
        assert_eq!(selected[1].file_path, PathBuf::from("b.rs"));
    }

    #[test]
    fn test_build_deep_analysis_guidance() {
        let review = MultiPassReview::with_defaults();
        let hotspot = HotspotResult {
            file_path: PathBuf::from("auth.rs"),
            line_range: (10, 50),
            risk_score: 0.85,
            reasons: vec!["Security-sensitive file path".to_string()],
            suggested_focus: vec![
                "Check for authentication vulnerabilities".to_string(),
            ],
        };

        let guidance = review.build_deep_analysis_guidance(&hotspot);
        assert!(guidance.contains("DEEP ANALYSIS MODE"));
        assert!(guidance.contains("0.85"));
        assert!(guidance.contains("Security-sensitive"));
        assert!(guidance.contains("10-50"));
    }

    #[test]
    fn test_merge_results_deduplicates() {
        let review = MultiPassReview::with_defaults();

        let first = vec![make_comment("a.rs", 10, "Missing null check on input")];
        let deep = vec![
            make_comment("a.rs", 10, "Missing null check on user input"), // similar
            make_comment("a.rs", 20, "Buffer overflow risk"),             // new
        ];

        let merged = review.merge_results(first, deep);
        // The similar comment should be deduplicated, new one should be added
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_results_boosts_deep_confidence() {
        let review = MultiPassReview::with_defaults();
        let first = vec![];
        let deep = vec![make_comment("a.rs", 5, "Unique deep finding")];

        let merged = review.merge_results(first, deep);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].confidence > 0.7); // boosted from 0.7
        assert!(merged[0].tags.contains(&"deep-analysis".to_string()));
    }

    #[test]
    fn test_content_similarity() {
        assert!(content_similarity("the cat sat on mat", "the cat sat on mat") > 0.99);
        assert!(content_similarity("the cat sat", "completely different words") < 0.2);
        assert!(content_similarity("", "") > 0.99);
        assert!(content_similarity("hello", "") < 0.01);
    }

    #[test]
    fn test_summarize_passes() {
        let review = MultiPassReview::with_defaults();
        let hotspots = vec![
            HotspotResult {
                file_path: PathBuf::from("a.rs"),
                line_range: (1, 10),
                risk_score: 0.8,
                reasons: vec![],
                suggested_focus: vec![],
            },
            HotspotResult {
                file_path: PathBuf::from("b.rs"),
                line_range: (1, 10),
                risk_score: 0.2,
                reasons: vec![],
                suggested_focus: vec![],
            },
        ];

        let summary = review.summarize_passes(&hotspots, 5, 3, 7);
        assert_eq!(summary.total_files_scanned, 2);
        assert_eq!(summary.high_risk_files, 1);
        assert_eq!(summary.first_pass_findings, 5);
        assert_eq!(summary.deep_pass_findings, 3);
        assert_eq!(summary.merged_findings, 7);
    }

    #[test]
    fn test_hotspot_is_high_risk() {
        let h = HotspotResult {
            file_path: PathBuf::from("test.rs"),
            line_range: (1, 10),
            risk_score: 0.6,
            reasons: vec![],
            suggested_focus: vec![],
        };
        assert!(h.is_high_risk(0.5));
        assert!(!h.is_high_risk(0.7));
    }

    #[test]
    fn test_hotspots_sorted_by_risk() {
        let review = MultiPassReview::with_defaults();
        let diffs = vec![
            make_diff("src/lib.rs", vec![make_added_line(1, "simple change")]),
            make_diff(
                "src/auth/crypto.rs",
                vec![make_added_line(1, "let secret = get_password();")],
            ),
        ];

        let hotspots = review.detect_hotspots(&diffs);
        if hotspots.len() >= 2 {
            assert!(hotspots[0].risk_score >= hotspots[1].risk_score);
        }
    }

    #[test]
    fn test_risk_score_clamped() {
        // A file with maximum risk factors
        let diffs = vec![make_diff(
            "src/auth/payment_migration.rs",
            (0..200)
                .map(|i| {
                    make_added_line(
                        i + 1,
                        "unsafe { eval(exec(sql_query(password, secret, shell))) }",
                    )
                })
                .collect(),
        )];

        let review = MultiPassReview::with_defaults();
        let hotspots = review.detect_hotspots(&diffs);
        assert!(!hotspots.is_empty());
        assert!(hotspots[0].risk_score <= 1.0);
    }

    #[test]
    fn test_multipass_config_default() {
        let config = MultiPassConfig::default();
        assert!(config.enable_hotspot_pass);
        assert!(config.enable_deep_pass);
        assert!((config.hotspot_threshold - 0.5).abs() < 0.01);
        assert_eq!(config.max_deep_files, 5);
    }
}
