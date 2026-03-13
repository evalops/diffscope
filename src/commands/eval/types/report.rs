use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::eval_benchmarks::{
    AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkThresholds, Difficulty,
    FixtureResult as BenchmarkFixtureResult,
};

use super::fixtures::EvalFixtureMetadata;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(in super::super) struct EvalRunFilters {
    #[serde(default)]
    pub(in super::super) suite_filters: Vec<String>,
    #[serde(default)]
    pub(in super::super) category_filters: Vec<String>,
    #[serde(default)]
    pub(in super::super) language_filters: Vec<String>,
    #[serde(default)]
    pub(in super::super) fixture_name_filters: Vec<String>,
    #[serde(default)]
    pub(in super::super) max_fixtures: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(in super::super) struct EvalRunMetadata {
    #[serde(default)]
    pub(in super::super) started_at: String,
    #[serde(default)]
    pub(in super::super) fixtures_root: String,
    #[serde(default)]
    pub(in super::super) fixtures_discovered: usize,
    #[serde(default)]
    pub(in super::super) fixtures_selected: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) label: Option<String>,
    #[serde(default)]
    pub(in super::super) model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) base_url: Option<String>,
    #[serde(default)]
    pub(in super::super) filters: EvalRunFilters,
    #[serde(default)]
    pub(in super::super) verification_fail_open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct EvalRuleMetrics {
    #[serde(default)]
    pub(in super::super) rule_id: String,
    #[serde(default)]
    pub(in super::super) expected: usize,
    #[serde(default)]
    pub(in super::super) predicted: usize,
    #[serde(default)]
    pub(in super::super) true_positives: usize,
    #[serde(default)]
    pub(in super::super) false_positives: usize,
    #[serde(default)]
    pub(in super::super) false_negatives: usize,
    #[serde(default)]
    pub(in super::super) precision: f32,
    #[serde(default)]
    pub(in super::super) recall: f32,
    #[serde(default)]
    pub(in super::super) f1: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub(in super::super) struct EvalRuleScoreSummary {
    #[serde(default)]
    pub(in super::super) micro_precision: f32,
    #[serde(default)]
    pub(in super::super) micro_recall: f32,
    #[serde(default)]
    pub(in super::super) micro_f1: f32,
    #[serde(default)]
    pub(in super::super) macro_precision: f32,
    #[serde(default)]
    pub(in super::super) macro_recall: f32,
    #[serde(default)]
    pub(in super::super) macro_f1: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct EvalFixtureResult {
    #[serde(default)]
    pub(in super::super) fixture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) suite: Option<String>,
    #[serde(default)]
    pub(in super::super) passed: bool,
    #[serde(default)]
    pub(in super::super) total_comments: usize,
    #[serde(default)]
    pub(in super::super) required_matches: usize,
    #[serde(default)]
    pub(in super::super) required_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) benchmark_metrics: Option<BenchmarkFixtureResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) suite_thresholds: Option<BenchmarkThresholds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) difficulty: Option<Difficulty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) metadata: Option<EvalFixtureMetadata>,
    #[serde(default)]
    pub(in super::super) rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub(in super::super) rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    pub(in super::super) warnings: Vec<String>,
    #[serde(default)]
    pub(in super::super) failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct EvalSuiteResult {
    #[serde(default)]
    pub(in super::super) suite: String,
    #[serde(default)]
    pub(in super::super) fixture_count: usize,
    #[serde(default)]
    pub(in super::super) aggregate: BenchmarkAggregateMetrics,
    #[serde(default)]
    pub(in super::super) thresholds_enforced: bool,
    #[serde(default)]
    pub(in super::super) threshold_pass: bool,
    #[serde(default)]
    pub(in super::super) threshold_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct EvalReport {
    #[serde(default)]
    pub(in super::super) run: EvalRunMetadata,
    #[serde(default)]
    pub(in super::super) fixtures_total: usize,
    #[serde(default)]
    pub(in super::super) fixtures_passed: usize,
    #[serde(default)]
    pub(in super::super) fixtures_failed: usize,
    #[serde(default)]
    pub(in super::super) rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub(in super::super) rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    pub(in super::super) suite_results: Vec<EvalSuiteResult>,
    #[serde(default)]
    pub(in super::super) benchmark_by_category: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub(in super::super) benchmark_by_language: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub(in super::super) benchmark_by_difficulty: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub(in super::super) warnings: Vec<String>,
    #[serde(default)]
    pub(in super::super) threshold_failures: Vec<String>,
    #[serde(default)]
    pub(in super::super) results: Vec<EvalFixtureResult>,
}
