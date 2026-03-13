use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::eval_benchmarks::{
    AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkThresholds, Difficulty,
    FixtureResult as BenchmarkFixtureResult,
};

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct EvalFixture {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) diff: Option<String>,
    #[serde(default)]
    pub(super) diff_file: Option<PathBuf>,
    #[serde(default)]
    pub(super) repo_path: Option<PathBuf>,
    #[serde(default)]
    pub(super) expect: EvalExpectations,
}

#[derive(Debug, Clone)]
pub(super) struct LoadedEvalFixture {
    pub(super) fixture_path: PathBuf,
    pub(super) fixture: EvalFixture,
    pub(super) suite_name: Option<String>,
    pub(super) suite_thresholds: Option<BenchmarkThresholds>,
    pub(super) difficulty: Option<Difficulty>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct EvalExpectations {
    #[serde(default)]
    pub(super) must_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(super) must_not_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(super) min_total: Option<usize>,
    #[serde(default)]
    pub(super) max_total: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct EvalPattern {
    #[serde(default)]
    pub(super) file: Option<String>,
    #[serde(default)]
    pub(super) line: Option<usize>,
    #[serde(default)]
    pub(super) contains: Option<String>,
    #[serde(default)]
    pub(super) contains_any: Vec<String>,
    #[serde(default)]
    pub(super) matches_regex: Option<String>,
    #[serde(default)]
    pub(super) severity: Option<String>,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default)]
    pub(super) tags_any: Vec<String>,
    #[serde(default)]
    pub(super) confidence_at_least: Option<f32>,
    #[serde(default)]
    pub(super) confidence_at_most: Option<f32>,
    #[serde(default)]
    pub(super) fix_effort: Option<String>,
    #[serde(default)]
    pub(super) rule_id: Option<String>,
    #[serde(default)]
    pub(super) require_rule_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct EvalRuleMetrics {
    #[serde(default)]
    pub(super) rule_id: String,
    #[serde(default)]
    pub(super) expected: usize,
    #[serde(default)]
    pub(super) predicted: usize,
    #[serde(default)]
    pub(super) true_positives: usize,
    #[serde(default)]
    pub(super) false_positives: usize,
    #[serde(default)]
    pub(super) false_negatives: usize,
    #[serde(default)]
    pub(super) precision: f32,
    #[serde(default)]
    pub(super) recall: f32,
    #[serde(default)]
    pub(super) f1: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub(super) struct EvalRuleScoreSummary {
    #[serde(default)]
    pub(super) micro_precision: f32,
    #[serde(default)]
    pub(super) micro_recall: f32,
    #[serde(default)]
    pub(super) micro_f1: f32,
    #[serde(default)]
    pub(super) macro_precision: f32,
    #[serde(default)]
    pub(super) macro_recall: f32,
    #[serde(default)]
    pub(super) macro_f1: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct EvalFixtureResult {
    #[serde(default)]
    pub(super) fixture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) suite: Option<String>,
    #[serde(default)]
    pub(super) passed: bool,
    #[serde(default)]
    pub(super) total_comments: usize,
    #[serde(default)]
    pub(super) required_matches: usize,
    #[serde(default)]
    pub(super) required_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) benchmark_metrics: Option<BenchmarkFixtureResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) suite_thresholds: Option<BenchmarkThresholds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) difficulty: Option<Difficulty>,
    #[serde(default)]
    pub(super) rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub(super) rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    pub(super) failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct EvalSuiteResult {
    #[serde(default)]
    pub(super) suite: String,
    #[serde(default)]
    pub(super) fixture_count: usize,
    #[serde(default)]
    pub(super) aggregate: BenchmarkAggregateMetrics,
    #[serde(default)]
    pub(super) thresholds_enforced: bool,
    #[serde(default)]
    pub(super) threshold_pass: bool,
    #[serde(default)]
    pub(super) threshold_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct EvalReport {
    #[serde(default)]
    pub(super) fixtures_total: usize,
    #[serde(default)]
    pub(super) fixtures_passed: usize,
    #[serde(default)]
    pub(super) fixtures_failed: usize,
    #[serde(default)]
    pub(super) rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub(super) rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    pub(super) suite_results: Vec<EvalSuiteResult>,
    #[serde(default)]
    pub(super) threshold_failures: Vec<String>,
    #[serde(default)]
    pub(super) results: Vec<EvalFixtureResult>,
}

#[derive(Debug, Clone)]
pub struct EvalRunOptions {
    pub baseline_report: Option<PathBuf>,
    pub max_micro_f1_drop: Option<f32>,
    pub min_micro_f1: Option<f32>,
    pub min_macro_f1: Option<f32>,
    pub min_rule_f1: Vec<String>,
    pub max_rule_f1_drop: Vec<String>,
}
