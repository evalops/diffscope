use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[path = "eval/fixtures.rs"]
mod fixtures;
#[path = "eval/metrics.rs"]
mod metrics;
#[path = "eval/pattern.rs"]
mod pattern;
#[path = "eval/runner.rs"]
mod runner;
#[path = "eval/thresholds.rs"]
mod thresholds;

use crate::config;
use crate::core::eval_benchmarks::{
    AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkThresholds, Difficulty,
    FixtureResult as BenchmarkFixtureResult,
};

use fixtures::{collect_eval_fixtures, load_eval_report};
use metrics::{
    aggregate_rule_metrics, build_suite_results, collect_suite_threshold_failures,
    summarize_rule_metrics,
};
use runner::run_eval_fixture;
use thresholds::{evaluate_eval_thresholds, parse_rule_threshold_args, EvalThresholdOptions};

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalFixture {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    diff: Option<String>,
    #[serde(default)]
    diff_file: Option<PathBuf>,
    #[serde(default)]
    repo_path: Option<PathBuf>,
    #[serde(default)]
    expect: EvalExpectations,
}

#[derive(Debug, Clone)]
struct LoadedEvalFixture {
    fixture_path: PathBuf,
    fixture: EvalFixture,
    suite_name: Option<String>,
    suite_thresholds: Option<BenchmarkThresholds>,
    difficulty: Option<Difficulty>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalExpectations {
    #[serde(default)]
    must_find: Vec<EvalPattern>,
    #[serde(default)]
    must_not_find: Vec<EvalPattern>,
    #[serde(default)]
    min_total: Option<usize>,
    #[serde(default)]
    max_total: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct EvalPattern {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<usize>,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    contains_any: Vec<String>,
    #[serde(default)]
    matches_regex: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags_any: Vec<String>,
    #[serde(default)]
    confidence_at_least: Option<f32>,
    #[serde(default)]
    confidence_at_most: Option<f32>,
    #[serde(default)]
    fix_effort: Option<String>,
    #[serde(default)]
    rule_id: Option<String>,
    #[serde(default)]
    require_rule_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalRuleMetrics {
    #[serde(default)]
    rule_id: String,
    #[serde(default)]
    expected: usize,
    #[serde(default)]
    predicted: usize,
    #[serde(default)]
    true_positives: usize,
    #[serde(default)]
    false_positives: usize,
    #[serde(default)]
    false_negatives: usize,
    #[serde(default)]
    precision: f32,
    #[serde(default)]
    recall: f32,
    #[serde(default)]
    f1: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct EvalRuleScoreSummary {
    #[serde(default)]
    micro_precision: f32,
    #[serde(default)]
    micro_recall: f32,
    #[serde(default)]
    micro_f1: f32,
    #[serde(default)]
    macro_precision: f32,
    #[serde(default)]
    macro_recall: f32,
    #[serde(default)]
    macro_f1: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalFixtureResult {
    #[serde(default)]
    fixture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suite: Option<String>,
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    total_comments: usize,
    #[serde(default)]
    required_matches: usize,
    #[serde(default)]
    required_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    benchmark_metrics: Option<BenchmarkFixtureResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suite_thresholds: Option<BenchmarkThresholds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    difficulty: Option<Difficulty>,
    #[serde(default)]
    rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalSuiteResult {
    #[serde(default)]
    suite: String,
    #[serde(default)]
    fixture_count: usize,
    #[serde(default)]
    aggregate: BenchmarkAggregateMetrics,
    #[serde(default)]
    thresholds_enforced: bool,
    #[serde(default)]
    threshold_pass: bool,
    #[serde(default)]
    threshold_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalReport {
    #[serde(default)]
    fixtures_total: usize,
    #[serde(default)]
    fixtures_passed: usize,
    #[serde(default)]
    fixtures_failed: usize,
    #[serde(default)]
    rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    suite_results: Vec<EvalSuiteResult>,
    #[serde(default)]
    threshold_failures: Vec<String>,
    #[serde(default)]
    results: Vec<EvalFixtureResult>,
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

pub async fn eval_command(
    config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
    options: EvalRunOptions,
) -> Result<()> {
    let fixtures = collect_eval_fixtures(&fixtures_dir)?;
    if fixtures.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let mut results = Vec::new();
    for fixture in fixtures {
        let result = run_eval_fixture(&config, fixture).await?;
        results.push(result);
    }

    let fixtures_total = results.len();
    let fixtures_passed = results.iter().filter(|result| result.passed).count();
    let fixtures_failed = fixtures_total.saturating_sub(fixtures_passed);
    let rule_metrics = aggregate_rule_metrics(&results);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
    let suite_results = build_suite_results(&results);
    let baseline = match options.baseline_report.as_deref() {
        Some(path) => Some(load_eval_report(path)?),
        None => None,
    };
    let min_rule_thresholds = parse_rule_threshold_args(&options.min_rule_f1, "min-rule-f1")?;
    let max_rule_drop_thresholds =
        parse_rule_threshold_args(&options.max_rule_f1_drop, "max-rule-f1-drop")?;
    let threshold_options = EvalThresholdOptions {
        max_micro_f1_drop: options.max_micro_f1_drop,
        min_micro_f1: options.min_micro_f1,
        min_macro_f1: options.min_macro_f1,
        min_rule_f1: min_rule_thresholds,
        max_rule_f1_drop: max_rule_drop_thresholds,
    };

    let mut report = EvalReport {
        fixtures_total,
        fixtures_passed,
        fixtures_failed,
        rule_metrics,
        rule_summary,
        suite_results,
        threshold_failures: Vec::new(),
        results,
    };
    let mut threshold_failures =
        evaluate_eval_thresholds(&report, baseline.as_ref(), &threshold_options);
    threshold_failures.extend(collect_suite_threshold_failures(&report.suite_results));
    report.threshold_failures = threshold_failures.clone();

    println!(
        "Eval summary: {}/{} fixture(s) passed",
        report.fixtures_passed, report.fixtures_total
    );
    for result in &report.results {
        if result.passed {
            println!(
                "[PASS] {} ({} comments, {}/{})",
                result.fixture,
                result.total_comments,
                result.required_matches,
                result.required_total
            );
        } else {
            println!(
                "[FAIL] {} ({} comments, {}/{})",
                result.fixture,
                result.total_comments,
                result.required_matches,
                result.required_total
            );
            for failure in &result.failures {
                println!("  - {}", failure);
            }
        }
        if let Some(rule_summary) = result.rule_summary {
            println!(
                "  rule-metrics: micro P={:.0}% R={:.0}% F1={:.0}%",
                rule_summary.micro_precision * 100.0,
                rule_summary.micro_recall * 100.0,
                rule_summary.micro_f1 * 100.0
            );
        }
    }

    if let Some(rule_summary) = report.rule_summary {
        println!(
            "Rule metrics (micro): P={:.0}% R={:.0}% F1={:.0}%",
            rule_summary.micro_precision * 100.0,
            rule_summary.micro_recall * 100.0,
            rule_summary.micro_f1 * 100.0
        );
        println!(
            "Rule metrics (macro): P={:.0}% R={:.0}% F1={:.0}%",
            rule_summary.macro_precision * 100.0,
            rule_summary.macro_recall * 100.0,
            rule_summary.macro_f1 * 100.0
        );

        for metric in report.rule_metrics.iter().take(8) {
            println!(
                "  - {}: tp={} fp={} fn={} (P={:.0}% R={:.0}%)",
                metric.rule_id,
                metric.true_positives,
                metric.false_positives,
                metric.false_negatives,
                metric.precision * 100.0,
                metric.recall * 100.0
            );
        }
    }
    for suite in &report.suite_results {
        println!(
            "Suite {}: fixtures={} micro F1={:.0}% weighted={:.0}%",
            suite.suite,
            suite.fixture_count,
            suite.aggregate.micro_f1 * 100.0,
            suite.aggregate.weighted_score * 100.0
        );
        if suite.thresholds_enforced {
            if suite.threshold_failures.is_empty() {
                println!("  suite-thresholds: passed");
            } else {
                for failure in &suite.threshold_failures {
                    println!("  suite-threshold-failure: {}", failure);
                }
            }
        }
    }
    for failure in &threshold_failures {
        println!("Threshold failure: {}", failure);
    }

    if let Some(path) = output_path {
        let serialized = serde_json::to_string_pretty(&report)?;
        tokio::fs::write(path, serialized).await?;
    }

    if report.fixtures_failed > 0 || !threshold_failures.is_empty() {
        let mut failure_parts = Vec::new();
        if report.fixtures_failed > 0 {
            failure_parts.push(format!(
                "{} fixture(s) did not meet expectations",
                report.fixtures_failed
            ));
        }
        if !threshold_failures.is_empty() {
            failure_parts.push(format!(
                "{} threshold check(s) failed",
                threshold_failures.len()
            ));
        }
        anyhow::bail!("Evaluation failed: {}", failure_parts.join("; "));
    }

    Ok(())
}
