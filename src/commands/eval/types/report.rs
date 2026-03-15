use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::dag::DagExecutionTrace;
use crate::core::eval_benchmarks::{
    AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkThresholds, Difficulty,
    FixtureResult as BenchmarkFixtureResult,
};
use crate::server::cost::CostBreakdownRow;

use super::fixtures::EvalFixtureMetadata;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalRunFilters {
    #[serde(default)]
    pub suite_filters: Vec<String>,
    #[serde(default)]
    pub category_filters: Vec<String>,
    #[serde(default)]
    pub language_filters: Vec<String>,
    #[serde(default)]
    pub fixture_name_filters: Vec<String>,
    #[serde(default)]
    pub max_fixtures: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalRunMetadata {
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub fixtures_root: String,
    #[serde(default)]
    pub fixtures_discovered: usize,
    #[serde(default)]
    pub fixtures_selected: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_group: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_model_role: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub review_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub filters: EvalRunFilters,
    #[serde(default)]
    pub verification_fail_open: bool,
    #[serde(default)]
    pub verification_judges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_consensus_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auditing_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auditing_model_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_total: Option<usize>,
    #[serde(default)]
    pub reproduction_validation: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cost_breakdowns: Vec<CostBreakdownRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalNamedMetricComparison {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub current_micro_f1: f32,
    #[serde(default)]
    pub baseline_micro_f1: f32,
    #[serde(default)]
    pub micro_f1_delta: f32,
    #[serde(default)]
    pub current_weighted_score: f32,
    #[serde(default)]
    pub baseline_weighted_score: f32,
    #[serde(default)]
    pub weighted_score_delta: f32,
    #[serde(default)]
    pub current_fixture_count: usize,
    #[serde(default)]
    pub baseline_fixture_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalVerificationHealth {
    #[serde(default)]
    pub verified_checks: usize,
    #[serde(default)]
    pub total_checks: usize,
    #[serde(default)]
    pub verified_pct: f32,
    #[serde(default)]
    pub warnings_total: usize,
    #[serde(default)]
    pub fixtures_with_warnings: usize,
    #[serde(default)]
    pub fail_open_warning_count: usize,
    #[serde(default)]
    pub parse_failure_count: usize,
    #[serde(default)]
    pub request_failure_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalAgentToolCall {
    #[serde(default)]
    pub iteration: usize,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalAgentActivity {
    #[serde(default)]
    pub total_iterations: usize,
    #[serde(default)]
    pub tool_calls: Vec<EvalAgentToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalVerificationJudgeReport {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub total_comments: usize,
    #[serde(default)]
    pub passed_comments: usize,
    #[serde(default)]
    pub filtered_comments: usize,
    #[serde(default)]
    pub abstained_comments: usize,
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub completion_tokens: usize,
    #[serde(default)]
    pub total_tokens: usize,
    #[serde(default)]
    pub cost_estimate_usd: f64,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalVerificationReport {
    #[serde(default)]
    pub consensus_mode: String,
    #[serde(default)]
    pub required_votes: usize,
    #[serde(default)]
    pub judge_count: usize,
    #[serde(default)]
    pub judges: Vec<EvalVerificationJudgeReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalReproductionCheck {
    #[serde(default)]
    pub comment_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reproduced: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default)]
    pub agent_activity: Option<EvalAgentActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalReproductionSummary {
    #[serde(default)]
    pub confirmed: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub inconclusive: usize,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub completion_tokens: usize,
    #[serde(default)]
    pub total_tokens: usize,
    #[serde(default)]
    pub cost_estimate_usd: f64,
    #[serde(default)]
    pub checks: Vec<EvalReproductionCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRuleMetrics {
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub expected: usize,
    #[serde(default)]
    pub predicted: usize,
    #[serde(default)]
    pub true_positives: usize,
    #[serde(default)]
    pub false_positives: usize,
    #[serde(default)]
    pub false_negatives: usize,
    #[serde(default)]
    pub precision: f32,
    #[serde(default)]
    pub recall: f32,
    #[serde(default)]
    pub f1: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct EvalRuleScoreSummary {
    #[serde(default)]
    pub micro_precision: f32,
    #[serde(default)]
    pub micro_recall: f32,
    #[serde(default)]
    pub micro_f1: f32,
    #[serde(default)]
    pub macro_precision: f32,
    #[serde(default)]
    pub macro_recall: f32,
    #[serde(default)]
    pub macro_f1: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalFixtureResult {
    #[serde(default)]
    pub fixture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub total_comments: usize,
    #[serde(default)]
    pub required_matches: usize,
    #[serde(default)]
    pub required_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_metrics: Option<BenchmarkFixtureResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_thresholds: Option<BenchmarkThresholds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<Difficulty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<EvalFixtureMetadata>,
    #[serde(default)]
    pub rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<EvalVerificationReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_activity: Option<EvalAgentActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reproduction_summary: Option<EvalReproductionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default)]
    pub failures: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cost_breakdowns: Vec<CostBreakdownRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dag_traces: Vec<DagExecutionTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSuiteResult {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub fixture_count: usize,
    #[serde(default)]
    pub aggregate: BenchmarkAggregateMetrics,
    #[serde(default)]
    pub thresholds_enforced: bool,
    #[serde(default)]
    pub threshold_pass: bool,
    #[serde(default)]
    pub threshold_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    #[serde(default)]
    pub run: EvalRunMetadata,
    #[serde(default)]
    pub fixtures_total: usize,
    #[serde(default)]
    pub fixtures_passed: usize,
    #[serde(default)]
    pub fixtures_failed: usize,
    #[serde(default)]
    pub rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    pub rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_summary: Option<BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub suite_results: Vec<EvalSuiteResult>,
    #[serde(default)]
    pub benchmark_by_category: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub benchmark_by_language: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub benchmark_by_difficulty: HashMap<String, BenchmarkAggregateMetrics>,
    #[serde(default)]
    pub suite_comparisons: Vec<EvalNamedMetricComparison>,
    #[serde(default)]
    pub category_comparisons: Vec<EvalNamedMetricComparison>,
    #[serde(default)]
    pub language_comparisons: Vec<EvalNamedMetricComparison>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_health: Option<EvalVerificationHealth>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub threshold_failures: Vec<String>,
    #[serde(default)]
    pub results: Vec<EvalFixtureResult>,
}
