use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::server::cost::CostBreakdownRow;

/// A benchmark suite with named fixture packs and quality thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub name: String,
    pub description: String,
    pub fixtures: Vec<BenchmarkFixture>,
    pub thresholds: BenchmarkThresholds,
    pub metadata: HashMap<String, String>,
}

impl BenchmarkSuite {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            fixtures: Vec::new(),
            thresholds: BenchmarkThresholds::default(),
            metadata: HashMap::new(),
        }
    }

    pub fn add_fixture(&mut self, fixture: BenchmarkFixture) {
        self.fixtures.push(fixture);
    }

    pub fn fixture_count(&self) -> usize {
        self.fixtures.len()
    }

    pub fn fixtures_by_category(&self) -> HashMap<String, Vec<&BenchmarkFixture>> {
        let mut by_category: HashMap<String, Vec<&BenchmarkFixture>> = HashMap::new();
        for fixture in &self.fixtures {
            by_category
                .entry(fixture.category.clone())
                .or_default()
                .push(fixture);
        }
        by_category
    }
}

/// A single benchmark fixture (diff + expected findings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkFixture {
    pub name: String,
    pub category: String,
    pub language: String,
    pub difficulty: Difficulty,
    pub diff_content: String,
    #[serde(default)]
    pub repo_path: Option<String>,
    pub expected_findings: Vec<ExpectedFinding>,
    pub negative_findings: Vec<NegativeFinding>,
    #[serde(default)]
    pub min_total: Option<usize>,
    #[serde(default)]
    pub max_total: Option<usize>,
    pub description: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    pub fn weight(&self) -> f32 {
        match self {
            Difficulty::Easy => 1.0,
            Difficulty::Medium => 1.5,
            Difficulty::Hard => 2.0,
            Difficulty::Expert => 3.0,
        }
    }
}

/// An expected finding that the reviewer should detect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedFinding {
    pub description: String,
    pub severity: Option<String>,
    pub category: Option<String>,
    pub file_pattern: Option<String>,
    pub line_hint: Option<usize>,
    pub contains: Option<String>,
    #[serde(default)]
    pub contains_any: Vec<String>,
    #[serde(default)]
    pub tags_any: Vec<String>,
    #[serde(default)]
    pub confidence_at_least: Option<f32>,
    #[serde(default)]
    pub confidence_at_most: Option<f32>,
    #[serde(default)]
    pub fix_effort: Option<String>,
    pub rule_id: Option<String>,
    #[serde(default)]
    pub rule_id_aliases: Vec<String>,
}

/// A finding that should NOT be reported (false positive check).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NegativeFinding {
    pub description: String,
    pub file_pattern: Option<String>,
    pub contains: Option<String>,
    #[serde(default)]
    pub contains_any: Vec<String>,
}

/// Quality thresholds for benchmark evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkThresholds {
    pub min_precision: f32,
    pub min_recall: f32,
    pub min_f1: f32,
    pub max_false_positive_rate: f32,
    pub min_weighted_score: f32,
}

impl Default for BenchmarkThresholds {
    fn default() -> Self {
        Self {
            min_precision: 0.5,
            min_recall: 0.4,
            min_f1: 0.4,
            max_false_positive_rate: 0.3,
            min_weighted_score: 0.5,
        }
    }
}

/// Result of running a benchmark suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub suite_name: String,
    pub fixture_results: Vec<FixtureResult>,
    pub aggregate: AggregateMetrics,
    pub by_category: HashMap<String, AggregateMetrics>,
    pub by_difficulty: HashMap<String, AggregateMetrics>,
    pub threshold_pass: bool,
    pub threshold_failures: Vec<String>,
    pub timestamp: String,
}

/// Result for a single fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureResult {
    pub fixture_name: String,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub true_negatives: usize,
    pub precision: f32,
    pub recall: f32,
    pub f1: f32,
    pub passed: bool,
    pub details: Vec<String>,
}

impl FixtureResult {
    pub fn compute(
        fixture_name: &str,
        expected_count: usize,
        negative_count: usize,
        matched_expected: usize,
        matched_negative: usize,
        extra_findings: usize,
    ) -> Self {
        let tp = matched_expected;
        let fp = extra_findings + matched_negative;
        let fn_ = expected_count.saturating_sub(matched_expected);
        let tn = negative_count.saturating_sub(matched_negative);

        let precision = if tp + fp > 0 {
            tp as f32 / (tp + fp) as f32
        } else {
            1.0
        };
        let recall = if tp + fn_ > 0 {
            tp as f32 / (tp + fn_) as f32
        } else {
            1.0
        };
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };

        let passed = fn_ == 0 && matched_negative == 0;

        Self {
            fixture_name: fixture_name.to_string(),
            true_positives: tp,
            false_positives: fp,
            false_negatives: fn_,
            true_negatives: tn,
            precision,
            recall,
            f1,
            passed,
            details: Vec::new(),
        }
    }
}

/// Aggregate metrics across multiple fixtures.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AggregateMetrics {
    pub fixture_count: usize,
    pub total_tp: usize,
    pub total_fp: usize,
    pub total_fn: usize,
    pub total_tn: usize,
    pub micro_precision: f32,
    pub micro_recall: f32,
    pub micro_f1: f32,
    pub macro_precision: f32,
    pub macro_recall: f32,
    pub macro_f1: f32,
    pub weighted_score: f32,
}

impl AggregateMetrics {
    pub fn compute(results: &[&FixtureResult], weights: Option<&[f32]>) -> Self {
        if results.is_empty() {
            return Self::default();
        }

        let total_tp: usize = results.iter().map(|r| r.true_positives).sum();
        let total_fp: usize = results.iter().map(|r| r.false_positives).sum();
        let total_fn: usize = results.iter().map(|r| r.false_negatives).sum();
        let total_tn: usize = results.iter().map(|r| r.true_negatives).sum();

        let micro_precision = if total_tp + total_fp > 0 {
            total_tp as f32 / (total_tp + total_fp) as f32
        } else {
            1.0
        };
        let micro_recall = if total_tp + total_fn > 0 {
            total_tp as f32 / (total_tp + total_fn) as f32
        } else {
            1.0
        };
        let micro_f1 = if micro_precision + micro_recall > 0.0 {
            2.0 * micro_precision * micro_recall / (micro_precision + micro_recall)
        } else {
            0.0
        };

        let n = results.len() as f32;
        let macro_precision: f32 = results.iter().map(|r| r.precision).sum::<f32>() / n;
        let macro_recall: f32 = results.iter().map(|r| r.recall).sum::<f32>() / n;
        let macro_f1: f32 = results.iter().map(|r| r.f1).sum::<f32>() / n;

        let weighted_score = if let Some(ws) = weights {
            if ws.len() != results.len() {
                // Length mismatch: fall back to macro_f1 rather than silently
                // dropping results via zip truncation
                macro_f1
            } else {
                let total_weight: f32 = ws.iter().sum();
                if total_weight > 0.0 {
                    results
                        .iter()
                        .zip(ws.iter())
                        .map(|(r, w)| r.f1 * w)
                        .sum::<f32>()
                        / total_weight
                } else {
                    macro_f1
                }
            }
        } else {
            macro_f1
        };

        AggregateMetrics {
            fixture_count: results.len(),
            total_tp,
            total_fp,
            total_fn,
            total_tn,
            micro_precision,
            micro_recall,
            micro_f1,
            macro_precision,
            macro_recall,
            macro_f1,
            weighted_score,
        }
    }
}

/// Evaluate a benchmark suite against thresholds.
pub fn evaluate_against_thresholds(
    result: &BenchmarkResult,
    thresholds: &BenchmarkThresholds,
) -> (bool, Vec<String>) {
    let mut failures = Vec::new();

    if result.aggregate.micro_precision < thresholds.min_precision {
        failures.push(format!(
            "Precision {:.3} < threshold {:.3}",
            result.aggregate.micro_precision, thresholds.min_precision
        ));
    }
    if result.aggregate.micro_recall < thresholds.min_recall {
        failures.push(format!(
            "Recall {:.3} < threshold {:.3}",
            result.aggregate.micro_recall, thresholds.min_recall
        ));
    }
    if result.aggregate.micro_f1 < thresholds.min_f1 {
        failures.push(format!(
            "F1 {:.3} < threshold {:.3}",
            result.aggregate.micro_f1, thresholds.min_f1
        ));
    }
    if result.aggregate.weighted_score < thresholds.min_weighted_score {
        failures.push(format!(
            "Weighted score {:.3} < threshold {:.3}",
            result.aggregate.weighted_score, thresholds.min_weighted_score
        ));
    }

    let fpr = if result.aggregate.total_fp + result.aggregate.total_tn > 0 {
        result.aggregate.total_fp as f32
            / (result.aggregate.total_fp + result.aggregate.total_tn) as f32
    } else {
        0.0
    };
    if fpr > thresholds.max_false_positive_rate {
        failures.push(format!(
            "False positive rate {:.3} > threshold {:.3}",
            fpr, thresholds.max_false_positive_rate
        ));
    }

    (failures.is_empty(), failures)
}

/// Compare two benchmark results to detect regressions.
pub fn compare_results(
    current: &BenchmarkResult,
    baseline: &BenchmarkResult,
    max_regression: f32,
) -> ComparisonResult {
    let f1_delta = current.aggregate.micro_f1 - baseline.aggregate.micro_f1;
    let precision_delta = current.aggregate.micro_precision - baseline.aggregate.micro_precision;
    let recall_delta = current.aggregate.micro_recall - baseline.aggregate.micro_recall;

    let mut regressions = Vec::new();
    if f1_delta < -max_regression {
        regressions.push(format!(
            "F1 regressed by {:.3} (was {:.3}, now {:.3})",
            -f1_delta, baseline.aggregate.micro_f1, current.aggregate.micro_f1
        ));
    }
    if precision_delta < -max_regression {
        regressions.push(format!("Precision regressed by {:.3}", -precision_delta));
    }
    if recall_delta < -max_regression {
        regressions.push(format!("Recall regressed by {:.3}", -recall_delta));
    }

    // Per-category regressions
    for (category, baseline_metrics) in &baseline.by_category {
        if let Some(current_metrics) = current.by_category.get(category) {
            let cat_f1_delta = current_metrics.micro_f1 - baseline_metrics.micro_f1;
            if cat_f1_delta < -max_regression {
                regressions.push(format!(
                    "Category '{}' F1 regressed by {:.3}",
                    category, -cat_f1_delta
                ));
            }
        }
    }

    let mut improvements = Vec::new();
    if f1_delta > max_regression {
        improvements.push(format!(
            "F1 improved by {:.3} (was {:.3}, now {:.3})",
            f1_delta, baseline.aggregate.micro_f1, current.aggregate.micro_f1
        ));
    }
    if precision_delta > max_regression {
        improvements.push(format!("Precision improved by {precision_delta:.3}"));
    }
    if recall_delta > max_regression {
        improvements.push(format!("Recall improved by {recall_delta:.3}"));
    }

    ComparisonResult {
        f1_delta,
        precision_delta,
        recall_delta,
        has_regression: !regressions.is_empty(),
        regressions,
        improvements,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    pub f1_delta: f32,
    pub precision_delta: f32,
    pub recall_delta: f32,
    pub has_regression: bool,
    pub regressions: Vec<String>,
    pub improvements: Vec<String>,
}

/// Track quality trends over multiple benchmark runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualityTrend {
    pub entries: Vec<TrendEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrendEntry {
    pub timestamp: String,
    pub micro_f1: f32,
    pub micro_precision: f32,
    pub micro_recall: f32,
    pub fixture_count: usize,
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weighted_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_accuracy: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usefulness_score: Option<f32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub suite_micro_f1: HashMap<String, f32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub category_micro_f1: HashMap<String, f32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub language_micro_f1: HashMap<String, f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_warning_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_fail_open_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_parse_failure_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_request_failure_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_verified_checks: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_total_checks: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_verified_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cost_breakdowns: Vec<CostBreakdownRow>,
}

impl QualityTrend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, result: &BenchmarkResult, label: Option<&str>) {
        self.entries.push(TrendEntry {
            timestamp: result.timestamp.clone(),
            micro_f1: result.aggregate.micro_f1,
            micro_precision: result.aggregate.micro_precision,
            micro_recall: result.aggregate.micro_recall,
            fixture_count: result.fixture_results.len(),
            label: label.map(|s| s.to_string()),
            weighted_score: Some(result.aggregate.weighted_score),
            ..Default::default()
        });
    }

    pub fn latest(&self) -> Option<&TrendEntry> {
        self.entries.last()
    }

    pub fn trend_direction(&self) -> TrendDirection {
        if self.entries.len() < 2 {
            return TrendDirection::Stable;
        }
        let recent = &self.entries[self.entries.len().saturating_sub(3)..];
        let first_f1 = recent.first().map(|e| e.micro_f1).unwrap_or(0.0);
        let last_f1 = recent.last().map(|e| e.micro_f1).unwrap_or(0.0);

        let delta = last_f1 - first_f1;
        if delta > 0.05 {
            TrendDirection::Improving
        } else if delta < -0.05 {
            TrendDirection::Degrading
        } else {
            TrendDirection::Stable
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrendDirection {
    Improving,
    Stable,
    Degrading,
}

/// A community-contributed fixture pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityFixturePack {
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub languages: Vec<String>,
    pub categories: Vec<String>,
    #[serde(default)]
    pub thresholds: Option<BenchmarkThresholds>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub fixtures: Vec<BenchmarkFixture>,
}

impl CommunityFixturePack {
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    pub fn to_benchmark_suite(&self) -> BenchmarkSuite {
        let mut suite = BenchmarkSuite::new(&self.name, &self.description);
        suite.fixtures = self.fixtures.clone();
        suite.thresholds = self.thresholds.clone().unwrap_or_default();
        suite.metadata.extend(self.metadata.clone());
        suite
            .metadata
            .insert("author".to_string(), self.author.clone());
        suite
            .metadata
            .insert("version".to_string(), self.version.clone());
        suite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fixture() -> BenchmarkFixture {
        BenchmarkFixture {
            name: "sql-injection-basic".to_string(),
            category: "security".to_string(),
            language: "python".to_string(),
            difficulty: Difficulty::Easy,
            diff_content: "def query(user_input):\n    db.execute(f\"SELECT * FROM users WHERE id={user_input}\")".to_string(),
            repo_path: None,
            expected_findings: vec![ExpectedFinding {
                description: "SQL injection via string interpolation".to_string(),
                severity: Some("Error".to_string()),
                category: Some("Security".to_string()),
                file_pattern: None,
                line_hint: Some(2),
                contains: Some("injection".to_string()),
                contains_any: Vec::new(),
                tags_any: Vec::new(),
                confidence_at_least: None,
                confidence_at_most: None,
                fix_effort: None,
                rule_id: Some("sec.sql.injection".to_string()),
                rule_id_aliases: Vec::new(),
            }],
            negative_findings: vec![],
            min_total: None,
            max_total: None,
            description: Some("Basic SQL injection detection".to_string()),
            source: Some("community".to_string()),
        }
    }

    #[test]
    fn test_benchmark_suite_creation() {
        let mut suite = BenchmarkSuite::new("security-basics", "Basic security checks");
        suite.add_fixture(sample_fixture());
        assert_eq!(suite.fixture_count(), 1);
        assert_eq!(suite.name, "security-basics");
    }

    #[test]
    fn test_fixtures_by_category() {
        let mut suite = BenchmarkSuite::new("mixed", "Mixed tests");
        suite.add_fixture(sample_fixture());
        suite.add_fixture(BenchmarkFixture {
            category: "performance".to_string(),
            ..sample_fixture()
        });

        let by_cat = suite.fixtures_by_category();
        assert_eq!(by_cat.len(), 2);
        assert_eq!(by_cat["security"].len(), 1);
        assert_eq!(by_cat["performance"].len(), 1);
    }

    #[test]
    fn test_fixture_result_compute() {
        let result = FixtureResult::compute("test", 5, 2, 4, 0, 1);
        assert_eq!(result.true_positives, 4);
        assert_eq!(result.false_positives, 1);
        assert_eq!(result.false_negatives, 1);
        assert!((result.precision - 0.8).abs() < 0.01);
        assert!((result.recall - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_fixture_result_perfect() {
        let result = FixtureResult::compute("perfect", 3, 1, 3, 0, 0);
        assert_eq!(result.true_positives, 3);
        assert_eq!(result.false_positives, 0);
        assert_eq!(result.false_negatives, 0);
        assert!((result.precision - 1.0).abs() < 0.01);
        assert!((result.recall - 1.0).abs() < 0.01);
        assert!((result.f1 - 1.0).abs() < 0.01);
        assert!(result.passed);
    }

    #[test]
    fn test_fixture_result_zero_findings() {
        let result = FixtureResult::compute("empty", 0, 0, 0, 0, 0);
        assert!((result.precision - 1.0).abs() < 0.01);
        assert!((result.recall - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_aggregate_metrics() {
        let r1 = FixtureResult::compute("a", 3, 1, 2, 0, 1);
        let r2 = FixtureResult::compute("b", 2, 1, 2, 0, 0);

        let aggregate = AggregateMetrics::compute(&[&r1, &r2], None);
        assert_eq!(aggregate.fixture_count, 2);
        assert_eq!(aggregate.total_tp, 4);
        assert_eq!(aggregate.total_fp, 1);
        assert_eq!(aggregate.total_fn, 1);
        assert!(aggregate.micro_f1 > 0.0);
    }

    #[test]
    fn test_aggregate_weighted() {
        let r1 = FixtureResult::compute("easy", 2, 0, 2, 0, 0); // perfect
        let r2 = FixtureResult::compute("hard", 5, 0, 1, 0, 3); // poor

        let weights = vec![1.0, 3.0]; // hard weighted 3x
        let aggregate = AggregateMetrics::compute(&[&r1, &r2], Some(&weights));

        // Weighted score should be pulled down by the hard fixture
        assert!(aggregate.weighted_score < aggregate.macro_f1 + 0.1);
    }

    #[test]
    fn test_evaluate_thresholds_pass() {
        let result = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![FixtureResult::compute("a", 3, 1, 3, 0, 0)],
            aggregate: AggregateMetrics {
                micro_precision: 0.9,
                micro_recall: 0.8,
                micro_f1: 0.85,
                weighted_score: 0.8,
                total_tp: 3,
                total_fp: 0,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };

        let (pass, failures) =
            evaluate_against_thresholds(&result, &BenchmarkThresholds::default());
        assert!(pass);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_evaluate_thresholds_fail() {
        let result = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![],
            aggregate: AggregateMetrics {
                micro_precision: 0.2,
                micro_recall: 0.1,
                micro_f1: 0.13,
                weighted_score: 0.1,
                total_tp: 1,
                total_fp: 5,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: false,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };

        let (pass, failures) =
            evaluate_against_thresholds(&result, &BenchmarkThresholds::default());
        assert!(!pass);
        assert!(!failures.is_empty());
    }

    #[test]
    fn test_compare_results_no_regression() {
        let baseline = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![],
            aggregate: AggregateMetrics {
                micro_f1: 0.8,
                micro_precision: 0.85,
                micro_recall: 0.75,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };
        let current = BenchmarkResult {
            aggregate: AggregateMetrics {
                micro_f1: 0.82,
                micro_precision: 0.87,
                micro_recall: 0.77,
                ..Default::default()
            },
            ..baseline.clone()
        };

        let comparison = compare_results(&current, &baseline, 0.1);
        assert!(!comparison.has_regression);
        assert!(comparison.f1_delta > 0.0);
    }

    #[test]
    fn test_compare_results_regression() {
        let baseline = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![],
            aggregate: AggregateMetrics {
                micro_f1: 0.8,
                micro_precision: 0.85,
                micro_recall: 0.75,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };
        let current = BenchmarkResult {
            aggregate: AggregateMetrics {
                micro_f1: 0.5,
                micro_precision: 0.55,
                micro_recall: 0.45,
                ..Default::default()
            },
            ..baseline.clone()
        };

        let comparison = compare_results(&current, &baseline, 0.1);
        assert!(comparison.has_regression);
        assert!(!comparison.regressions.is_empty());
    }

    #[test]
    fn test_quality_trend() {
        let mut trend = QualityTrend::new();
        assert_eq!(trend.trend_direction(), TrendDirection::Stable);

        for i in 0..5 {
            trend.entries.push(TrendEntry {
                timestamp: format!("2024-01-0{}", i + 1),
                micro_f1: 0.5 + (i as f32 * 0.05),
                micro_precision: 0.5,
                micro_recall: 0.5,
                fixture_count: 10,
                label: None,
                ..Default::default()
            });
        }

        assert_eq!(trend.trend_direction(), TrendDirection::Improving);
        assert!(trend.latest().is_some());
    }

    #[test]
    fn test_quality_trend_degrading() {
        let mut trend = QualityTrend::new();
        for i in 0..5 {
            trend.entries.push(TrendEntry {
                timestamp: format!("2024-01-0{}", i + 1),
                micro_f1: 0.9 - (i as f32 * 0.05),
                micro_precision: 0.5,
                micro_recall: 0.5,
                fixture_count: 10,
                label: None,
                ..Default::default()
            });
        }

        assert_eq!(trend.trend_direction(), TrendDirection::Degrading);
    }

    #[test]
    fn test_quality_trend_record() {
        let mut trend = QualityTrend::new();
        assert!(trend.entries.is_empty());

        let result = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![
                FixtureResult::compute("a", 3, 1, 3, 0, 0),
                FixtureResult::compute("b", 2, 1, 2, 0, 0),
            ],
            aggregate: AggregateMetrics {
                micro_precision: 0.9,
                micro_recall: 0.85,
                micro_f1: 0.87,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-06-15".to_string(),
        };

        trend.record(&result, Some("v2.0"));
        assert_eq!(trend.entries.len(), 1);
        assert_eq!(trend.entries[0].timestamp, "2024-06-15");
        assert!((trend.entries[0].micro_f1 - 0.87).abs() < 0.01);
        assert_eq!(trend.entries[0].fixture_count, 2);
        assert_eq!(trend.entries[0].label.as_deref(), Some("v2.0"));

        trend.record(&result, None);
        assert_eq!(trend.entries.len(), 2);
        assert!(trend.entries[1].label.is_none());
    }

    #[test]
    fn test_quality_trend_serialization() {
        let mut trend = QualityTrend::new();
        trend.entries.push(TrendEntry {
            timestamp: "2024-01-01".to_string(),
            micro_f1: 0.75,
            micro_precision: 0.8,
            micro_recall: 0.7,
            fixture_count: 10,
            label: Some("v1.0".to_string()),
            ..Default::default()
        });

        let json = trend.to_json().unwrap();
        let restored = QualityTrend::from_json(&json).unwrap();
        assert_eq!(restored.entries.len(), 1);
        assert_eq!(restored.entries[0].label.as_deref(), Some("v1.0"));
    }

    #[test]
    fn test_community_fixture_pack() {
        let json = serde_json::to_string(&CommunityFixturePack {
            name: "owasp-top10".to_string(),
            author: "community".to_string(),
            version: "1.0.0".to_string(),
            description: "OWASP Top 10 vulnerability checks".to_string(),
            languages: vec!["python".to_string(), "javascript".to_string()],
            categories: vec!["security".to_string()],
            thresholds: Some(BenchmarkThresholds {
                min_precision: 0.8,
                min_recall: 0.7,
                min_f1: 0.75,
                max_false_positive_rate: 0.1,
                min_weighted_score: 0.77,
            }),
            metadata: HashMap::from([("source".to_string(), "community-pack".to_string())]),
            fixtures: vec![sample_fixture()],
        })
        .unwrap();

        let pack = CommunityFixturePack::from_json(&json).unwrap();
        assert_eq!(pack.name, "owasp-top10");
        assert_eq!(pack.fixtures.len(), 1);

        let suite = pack.to_benchmark_suite();
        assert_eq!(suite.name, "owasp-top10");
        assert_eq!(suite.metadata.get("author").unwrap(), "community");
        assert_eq!(suite.metadata.get("source").unwrap(), "community-pack");
        assert!((suite.thresholds.min_precision - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_difficulty_weights() {
        assert!(Difficulty::Expert.weight() > Difficulty::Hard.weight());
        assert!(Difficulty::Hard.weight() > Difficulty::Medium.weight());
        assert!(Difficulty::Medium.weight() > Difficulty::Easy.weight());
    }

    #[test]
    fn test_default_thresholds() {
        let t = BenchmarkThresholds::default();
        assert!(t.min_f1 > 0.0);
        assert!(t.min_precision > 0.0);
        assert!(t.max_false_positive_rate > 0.0);
    }

    #[test]
    fn test_fixture_result_all_zeros() {
        let result = FixtureResult::compute("zero", 0, 0, 0, 0, 0);
        // No TPs, no FPs, no FNs — precision and recall default to 1.0
        assert!((result.precision - 1.0).abs() < f32::EPSILON);
        assert!((result.recall - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fixture_result_perfect_score() {
        let result = FixtureResult::compute("perfect", 5, 0, 5, 0, 0);
        assert!((result.precision - 1.0).abs() < f32::EPSILON);
        assert!((result.recall - 1.0).abs() < f32::EPSILON);
        assert!((result.f1 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_fixture_result_no_true_positives() {
        let result = FixtureResult::compute("bad", 5, 0, 0, 0, 5);
        assert!((result.precision).abs() < f32::EPSILON);
        assert!((result.recall).abs() < f32::EPSILON);
    }

    #[test]
    fn test_aggregate_metrics_empty() {
        let agg = AggregateMetrics::compute(&[], None);
        assert_eq!(agg.fixture_count, 0);
    }

    #[test]
    fn test_aggregate_metrics_single_fixture() {
        let result = FixtureResult::compute("single", 10, 0, 8, 0, 2);
        let agg = AggregateMetrics::compute(&[&result], None);
        assert_eq!(agg.fixture_count, 1);
        assert!(agg.micro_precision > 0.0);
        assert!(agg.micro_recall > 0.0);
    }

    #[test]
    fn test_empty_suite_evaluation() {
        let suite = BenchmarkSuite::new("empty", "No fixtures");
        assert_eq!(suite.fixture_count(), 0);
        let by_cat = suite.fixtures_by_category();
        assert!(by_cat.is_empty());
    }

    // Regression: mismatched weights must fall back to macro_f1, not silently drop results
    #[test]
    fn test_aggregate_weights_length_mismatch() {
        let r1 = FixtureResult::compute("a", 2, 0, 2, 0, 0); // f1 = 1.0
        let r2 = FixtureResult::compute("b", 2, 0, 0, 0, 2); // f1 = 0.0
        let r3 = FixtureResult::compute("c", 2, 0, 2, 0, 0); // f1 = 1.0

        // Only 2 weights for 3 results — r3 (f1=1.0) is silently dropped
        let weights = vec![1.0, 1.0];
        let agg = AggregateMetrics::compute(&[&r1, &r2, &r3], Some(&weights));

        // macro_f1 = (1.0 + 0.0 + 1.0) / 3 = 0.667
        // With mismatched weights, zip truncates: (1.0*1 + 0.0*1) / 2 = 0.5
        // The third fixture is silently ignored, deflating the score!
        // On mismatch, should fall back to macro_f1 rather than give wrong answer.
        assert!(
            (agg.weighted_score - agg.macro_f1).abs() < 0.05,
            "Mismatched weights should fall back to macro_f1, got weighted={:.3} vs macro={:.3}",
            agg.weighted_score,
            agg.macro_f1
        );
    }

    // Regression: FPR must use FP/(FP+TN), not FP/(FP+TP)
    #[test]
    fn test_fpr_uses_true_negatives() {
        let r1 = FixtureResult::compute("a", 10, 10, 10, 0, 1);
        // r1: tp=10, fp=1, fn=0, tn=10
        let r2 = FixtureResult::compute("b", 0, 10, 0, 0, 0);
        // r2: tp=0, fp=0, fn=0, tn=10

        let agg = AggregateMetrics::compute(&[&r1, &r2], None);
        // total_tp=10, total_fp=1, total_fn=0
        // If total_tn were tracked: total_tn=20

        let result = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![r1, r2],
            aggregate: agg,
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };

        // With real FPR = FP/(FP+TN) = 1/(1+20) = 0.048 → passes 0.06 threshold
        // With buggy FPR = FP/(FP+TP) = 1/(1+10) = 0.091 → fails 0.06 threshold
        let thresholds = BenchmarkThresholds {
            max_false_positive_rate: 0.06,
            min_precision: 0.0,
            min_recall: 0.0,
            min_f1: 0.0,
            min_weighted_score: 0.0,
        };

        let (pass, failures) = evaluate_against_thresholds(&result, &thresholds);
        assert!(
            pass,
            "FPR should be ~0.048 (FP/(FP+TN)), not 0.091 (FP/(FP+TP)): {failures:?}"
        );
    }

    // Regression: compare_results must populate improvements when metrics improve
    #[test]
    fn test_compare_results_detects_improvements() {
        let baseline = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![],
            aggregate: AggregateMetrics {
                micro_f1: 0.5,
                micro_precision: 0.5,
                micro_recall: 0.5,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-01".to_string(),
        };
        let current = BenchmarkResult {
            suite_name: "test".to_string(),
            fixture_results: vec![],
            aggregate: AggregateMetrics {
                micro_f1: 0.9,
                micro_precision: 0.9,
                micro_recall: 0.9,
                ..Default::default()
            },
            by_category: HashMap::new(),
            by_difficulty: HashMap::new(),
            threshold_pass: true,
            threshold_failures: vec![],
            timestamp: "2024-01-02".to_string(),
        };

        let comparison = compare_results(&current, &baseline, 0.05);
        assert!(!comparison.has_regression);
        assert!(
            !comparison.improvements.is_empty(),
            "Should detect F1 improvement of +0.4"
        );
    }
}
