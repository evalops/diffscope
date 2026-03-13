use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config;
use crate::core;
use crate::core::eval_benchmarks::{
    evaluate_against_thresholds, AggregateMetrics as BenchmarkAggregateMetrics, BenchmarkResult,
    BenchmarkThresholds, CommunityFixturePack, Difficulty, FixtureResult as BenchmarkFixtureResult,
};
use crate::review::{normalize_rule_id, review_diff_content_raw};

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

#[derive(Debug, Clone)]
struct EvalThresholdOptions {
    max_micro_f1_drop: Option<f32>,
    min_micro_f1: Option<f32>,
    min_macro_f1: Option<f32>,
    min_rule_f1: Vec<EvalRuleThreshold>,
    max_rule_f1_drop: Vec<EvalRuleThreshold>,
}

#[derive(Debug, Clone)]
struct EvalRuleThreshold {
    rule_id: String,
    value: f32,
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

fn collect_fixture_paths(fixtures_dir: &Path) -> Result<Vec<PathBuf>> {
    if !fixtures_dir.exists() {
        anyhow::bail!("Fixtures directory not found: {}", fixtures_dir.display());
    }
    if !fixtures_dir.is_dir() {
        anyhow::bail!(
            "Fixtures path is not a directory: {}",
            fixtures_dir.display()
        );
    }

    let mut paths = Vec::new();
    let mut stack = vec![fixtures_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if matches!(extension.as_deref(), Some("json" | "yml" | "yaml")) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

fn collect_eval_fixtures(fixtures_dir: &Path) -> Result<Vec<LoadedEvalFixture>> {
    let mut fixtures = Vec::new();
    for path in collect_fixture_paths(fixtures_dir)? {
        fixtures.extend(load_eval_fixtures_from_path(&path)?);
    }
    fixtures.sort_by(|left, right| {
        left.fixture_path
            .cmp(&right.fixture_path)
            .then_with(|| left.fixture.name.cmp(&right.fixture.name))
    });
    Ok(fixtures)
}

fn load_eval_fixtures_from_path(path: &Path) -> Result<Vec<LoadedEvalFixture>> {
    let content = std::fs::read_to_string(path)?;

    if let Ok(pack) = load_fixture_file::<CommunityFixturePack>(path, &content) {
        return expand_community_fixture_pack(path, pack);
    }

    let fixture = load_eval_fixture_from_content(path, &content)?;
    Ok(vec![LoadedEvalFixture {
        fixture_path: path.to_path_buf(),
        fixture,
        suite_name: None,
        suite_thresholds: None,
        difficulty: None,
    }])
}

fn load_eval_fixture_from_content(path: &Path, content: &str) -> Result<EvalFixture> {
    let fixture = load_fixture_file::<EvalFixture>(path, content)?;
    validate_eval_fixture(&fixture)?;
    Ok(fixture)
}

fn load_fixture_file<T>(path: &Path, content: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Ok(serde_json::from_str(content)?),
        _ => match serde_yaml::from_str(content) {
            Ok(parsed) => Ok(parsed),
            Err(_) => Ok(serde_json::from_str(content)?),
        },
    }
}

fn expand_community_fixture_pack(
    path: &Path,
    pack: CommunityFixturePack,
) -> Result<Vec<LoadedEvalFixture>> {
    let pack_name = pack.name;
    let thresholds = pack.thresholds;
    pack.fixtures
        .into_iter()
        .map(|fixture| {
            let difficulty = fixture.difficulty.clone();
            let eval_fixture = EvalFixture {
                name: Some(format!("{}/{}", pack_name, fixture.name)),
                diff: Some(fixture.diff_content),
                diff_file: None,
                repo_path: None,
                expect: EvalExpectations {
                    must_find: fixture
                        .expected_findings
                        .into_iter()
                        .map(|finding| EvalPattern {
                            file: finding.file_pattern,
                            line: finding.line_hint,
                            contains: finding.contains,
                            severity: finding.severity,
                            category: finding.category,
                            rule_id: finding.rule_id.clone(),
                            require_rule_id: finding.rule_id.is_some(),
                            ..Default::default()
                        })
                        .collect(),
                    must_not_find: fixture
                        .negative_findings
                        .into_iter()
                        .map(|finding| EvalPattern {
                            file: finding.file_pattern,
                            contains: finding.contains,
                            ..Default::default()
                        })
                        .collect(),
                    min_total: None,
                    max_total: None,
                },
            };
            validate_eval_fixture(&eval_fixture)?;

            Ok(LoadedEvalFixture {
                fixture_path: path.to_path_buf(),
                fixture: eval_fixture,
                suite_name: Some(pack_name.clone()),
                suite_thresholds: thresholds.clone(),
                difficulty: Some(difficulty),
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn validate_eval_fixture(fixture: &EvalFixture) -> Result<()> {
    for pattern in fixture
        .expect
        .must_find
        .iter()
        .chain(fixture.expect.must_not_find.iter())
    {
        if let Some(pattern_text) = pattern.matches_regex.as_deref().map(str::trim) {
            if !pattern_text.is_empty() {
                Regex::new(pattern_text).map_err(|error| {
                    anyhow::anyhow!(
                        "Invalid regex '{}' in fixture '{}': {}",
                        pattern_text,
                        fixture.name.as_deref().unwrap_or("<unnamed>"),
                        error
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn load_eval_report(path: &Path) -> Result<EvalReport> {
    let content = std::fs::read_to_string(path)?;
    let report: EvalReport = serde_json::from_str(&content)?;
    Ok(report)
}

fn parse_rule_threshold_args(values: &[String], label: &str) -> Result<Vec<EvalRuleThreshold>> {
    let mut parsed = Vec::new();
    for raw in values {
        let Some((rule_id, value)) = raw.split_once('=') else {
            anyhow::bail!("Invalid {} entry '{}': expected rule_id=value", label, raw);
        };
        let rule_id = rule_id.trim().to_ascii_lowercase();
        if rule_id.is_empty() {
            anyhow::bail!("Invalid {} entry '{}': empty rule id", label, raw);
        }
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid {} entry '{}': invalid float", label, raw))?;
        if !(0.0..=1.0).contains(&value) {
            anyhow::bail!(
                "Invalid {} entry '{}': value must be between 0.0 and 1.0",
                label,
                raw
            );
        }
        parsed.push(EvalRuleThreshold { rule_id, value });
    }
    Ok(parsed)
}

fn evaluate_eval_thresholds(
    current: &EvalReport,
    baseline: Option<&EvalReport>,
    options: &EvalThresholdOptions,
) -> Vec<String> {
    let mut failures = Vec::new();
    let current_micro_f1 = current
        .rule_summary
        .map(|summary| summary.micro_f1)
        .unwrap_or(0.0);
    let current_macro_f1 = current
        .rule_summary
        .map(|summary| summary.macro_f1)
        .unwrap_or(0.0);

    if let Some(threshold) = options.min_micro_f1 {
        let threshold = threshold.clamp(0.0, 1.0);
        if current_micro_f1 < threshold {
            failures.push(format!(
                "micro-F1 {:.3} is below minimum {:.3}",
                current_micro_f1, threshold
            ));
        }
    }

    if let Some(threshold) = options.min_macro_f1 {
        let threshold = threshold.clamp(0.0, 1.0);
        if current_macro_f1 < threshold {
            failures.push(format!(
                "macro-F1 {:.3} is below minimum {:.3}",
                current_macro_f1, threshold
            ));
        }
    }

    let current_by_rule = build_rule_f1_map(&current.rule_metrics);
    for threshold in &options.min_rule_f1 {
        let current = current_by_rule
            .get(&threshold.rule_id)
            .copied()
            .unwrap_or(0.0);
        if current < threshold.value {
            failures.push(format!(
                "rule '{}' F1 {:.3} is below minimum {:.3}",
                threshold.rule_id, current, threshold.value
            ));
        }
    }

    if options.max_micro_f1_drop.is_some() || !options.max_rule_f1_drop.is_empty() {
        let Some(baseline) = baseline else {
            failures.push(
                "baseline report is required for drop-based thresholds (--baseline)".to_string(),
            );
            return failures;
        };

        let baseline_summary = baseline.rule_summary.unwrap_or_default();
        if let Some(max_drop) = options.max_micro_f1_drop {
            let max_drop = max_drop.clamp(0.0, 1.0);
            let drop = (baseline_summary.micro_f1 - current_micro_f1).max(0.0);
            if drop > max_drop {
                failures.push(format!(
                    "micro-F1 drop {:.3} exceeded max {:.3} (baseline {:.3} -> current {:.3})",
                    drop, max_drop, baseline_summary.micro_f1, current_micro_f1
                ));
            }
        }

        if !options.max_rule_f1_drop.is_empty() {
            let baseline_by_rule = build_rule_f1_map(&baseline.rule_metrics);
            for threshold in &options.max_rule_f1_drop {
                let baseline_f1 = baseline_by_rule
                    .get(&threshold.rule_id)
                    .copied()
                    .unwrap_or(0.0);
                let current_f1 = current_by_rule
                    .get(&threshold.rule_id)
                    .copied()
                    .unwrap_or(0.0);
                let drop = (baseline_f1 - current_f1).max(0.0);
                if drop > threshold.value {
                    failures.push(format!(
                        "rule '{}' F1 drop {:.3} exceeded max {:.3} (baseline {:.3} -> current {:.3})",
                        threshold.rule_id, drop, threshold.value, baseline_f1, current_f1
                    ));
                }
            }
        }
    }

    failures
}

fn build_rule_f1_map(metrics: &[EvalRuleMetrics]) -> HashMap<String, f32> {
    let mut by_rule = HashMap::new();
    for metric in metrics {
        by_rule.insert(metric.rule_id.to_ascii_lowercase(), metric.f1);
    }
    by_rule
}

fn build_suite_results(results: &[EvalFixtureResult]) -> Vec<EvalSuiteResult> {
    let mut grouped: HashMap<String, Vec<&EvalFixtureResult>> = HashMap::new();
    for result in results {
        if let (Some(suite), Some(_)) = (&result.suite, &result.benchmark_metrics) {
            grouped.entry(suite.clone()).or_default().push(result);
        }
    }

    let mut suites = Vec::new();
    for (suite_name, suite_results) in grouped {
        let mut fixture_results = Vec::new();
        let mut weights = Vec::new();
        let mut thresholds = None;

        for result in suite_results {
            if let Some(metrics) = result.benchmark_metrics.as_ref() {
                fixture_results.push(metrics);
                weights.push(
                    result
                        .difficulty
                        .as_ref()
                        .map(Difficulty::weight)
                        .unwrap_or(1.0),
                );
                if thresholds.is_none() {
                    thresholds = result.suite_thresholds.clone();
                }
            }
        }

        let aggregate = BenchmarkAggregateMetrics::compute(&fixture_results, Some(&weights));
        let (thresholds_enforced, threshold_pass, threshold_failures) =
            if let Some(thresholds) = thresholds.as_ref() {
                let benchmark_result = BenchmarkResult {
                    suite_name: suite_name.clone(),
                    fixture_results: fixture_results
                        .iter()
                        .map(|result| (*result).clone())
                        .collect(),
                    aggregate: aggregate.clone(),
                    by_category: HashMap::new(),
                    by_difficulty: HashMap::new(),
                    threshold_pass: true,
                    threshold_failures: Vec::new(),
                    timestamp: String::new(),
                };
                let (passed, failures) = evaluate_against_thresholds(&benchmark_result, thresholds);
                (true, passed, failures)
            } else {
                (false, true, Vec::new())
            };

        suites.push(EvalSuiteResult {
            suite: suite_name,
            fixture_count: fixture_results.len(),
            aggregate,
            thresholds_enforced,
            threshold_pass,
            threshold_failures,
        });
    }

    suites.sort_by(|left, right| left.suite.cmp(&right.suite));
    suites
}

fn collect_suite_threshold_failures(suites: &[EvalSuiteResult]) -> Vec<String> {
    let mut failures = Vec::new();
    for suite in suites {
        for failure in &suite.threshold_failures {
            failures.push(format!("suite '{}' {}", suite.suite, failure));
        }
    }
    failures
}

async fn run_eval_fixture(
    config: &config::Config,
    loaded_fixture: LoadedEvalFixture,
) -> Result<EvalFixtureResult> {
    let LoadedEvalFixture {
        fixture_path,
        fixture,
        suite_name,
        suite_thresholds,
        difficulty,
    } = loaded_fixture;
    let fixture_name = fixture.name.unwrap_or_else(|| {
        fixture_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("fixture")
            .to_string()
    });
    let fixture_dir = fixture_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let diff_content = match (fixture.diff, fixture.diff_file) {
        (Some(diff), _) => diff,
        (None, Some(diff_file)) => {
            let path = if diff_file.is_absolute() {
                diff_file
            } else {
                fixture_dir.join(diff_file)
            };
            std::fs::read_to_string(path)?
        }
        (None, None) => anyhow::bail!(
            "Fixture '{}' must define either diff or diff_file",
            fixture_name
        ),
    };

    let repo_path = fixture
        .repo_path
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                fixture_dir.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let review_result = review_diff_content_raw(&diff_content, config.clone(), &repo_path).await?;
    let comments = review_result.comments;
    let total_comments = comments.len();
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let required_total = fixture.expect.must_find.len();
    let mut used_comment_indices = HashSet::new();
    let mut unexpected_comment_indices = HashSet::new();
    let mut matched_pairs = Vec::new();

    for (expected_idx, expected) in fixture.expect.must_find.iter().enumerate() {
        let found = comments
            .iter()
            .enumerate()
            .find(|(comment_idx, comment)| {
                !used_comment_indices.contains(comment_idx) && expected.matches(comment)
            })
            .map(|(comment_idx, _)| comment_idx);

        if let Some(comment_idx) = found {
            used_comment_indices.insert(comment_idx);
            matched_pairs.push((expected_idx, comment_idx));
            required_matches = required_matches.saturating_add(1);
        } else {
            failures.push(format!("Missing expected finding: {}", expected.describe()));
        }
    }

    for unexpected in &fixture.expect.must_not_find {
        if let Some((comment_idx, comment)) = comments
            .iter()
            .enumerate()
            .find(|(_, comment)| unexpected.matches(comment))
        {
            unexpected_comment_indices.insert(comment_idx);
            failures.push(format!(
                "Unexpected finding matched {}:{} '{}'",
                comment.file_path.display(),
                comment.line_number,
                summarize_for_eval(&comment.content)
            ));
        }
    }

    let rule_metrics = compute_rule_metrics(&fixture.expect.must_find, &comments, &matched_pairs);
    let rule_summary = summarize_rule_metrics(&rule_metrics);

    if let Some(min_total) = fixture.expect.min_total {
        if total_comments < min_total {
            failures.push(format!(
                "Expected at least {} comments, got {}",
                min_total, total_comments
            ));
        }
    }
    if let Some(max_total) = fixture.expect.max_total {
        if total_comments > max_total {
            failures.push(format!(
                "Expected at most {} comments, got {}",
                max_total, total_comments
            ));
        }
    }

    let benchmark_metrics = suite_name.as_ref().map(|_| {
        let accounted_for = used_comment_indices
            .union(&unexpected_comment_indices)
            .count();
        let extra_findings = total_comments.saturating_sub(accounted_for);
        let mut result = BenchmarkFixtureResult::compute(
            &fixture_name,
            fixture.expect.must_find.len(),
            fixture.expect.must_not_find.len(),
            required_matches,
            unexpected_comment_indices.len(),
            extra_findings,
        );
        result.details = failures.clone();
        result
    });

    Ok(EvalFixtureResult {
        fixture: fixture_name,
        suite: suite_name,
        passed: failures.is_empty(),
        total_comments,
        required_matches,
        required_total,
        benchmark_metrics,
        suite_thresholds,
        difficulty,
        rule_metrics,
        rule_summary,
        failures,
    })
}

#[derive(Debug, Default, Clone, Copy)]
struct RuleMetricCounts {
    expected: usize,
    predicted: usize,
    true_positives: usize,
}

fn compute_rule_metrics(
    expected_patterns: &[EvalPattern],
    comments: &[core::Comment],
    matched_pairs: &[(usize, usize)],
) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();

    for pattern in expected_patterns {
        if let Some(rule_id) = pattern.normalized_rule_id() {
            counts_by_rule.entry(rule_id).or_default().expected += 1;
        }
    }

    for comment in comments {
        if let Some(rule_id) = normalize_rule_id(comment.rule_id.as_deref()) {
            counts_by_rule.entry(rule_id).or_default().predicted += 1;
        }
    }

    for (expected_idx, comment_idx) in matched_pairs {
        let expected_rule = expected_patterns
            .get(*expected_idx)
            .and_then(EvalPattern::normalized_rule_id);
        let predicted_rule = comments
            .get(*comment_idx)
            .and_then(|comment| normalize_rule_id(comment.rule_id.as_deref()));
        if let (Some(expected_rule), Some(predicted_rule)) = (expected_rule, predicted_rule) {
            if expected_rule == predicted_rule {
                counts_by_rule
                    .entry(expected_rule)
                    .or_default()
                    .true_positives += 1;
            }
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

fn aggregate_rule_metrics(results: &[EvalFixtureResult]) -> Vec<EvalRuleMetrics> {
    let mut counts_by_rule: HashMap<String, RuleMetricCounts> = HashMap::new();
    for result in results {
        for metric in &result.rule_metrics {
            let counts = counts_by_rule.entry(metric.rule_id.clone()).or_default();
            counts.expected = counts.expected.saturating_add(metric.expected);
            counts.predicted = counts.predicted.saturating_add(metric.predicted);
            counts.true_positives = counts.true_positives.saturating_add(metric.true_positives);
        }
    }

    build_rule_metrics_from_counts(&counts_by_rule)
}

fn build_rule_metrics_from_counts(
    counts_by_rule: &HashMap<String, RuleMetricCounts>,
) -> Vec<EvalRuleMetrics> {
    let mut metrics = Vec::new();
    for (rule_id, counts) in counts_by_rule {
        let false_positives = counts.predicted.saturating_sub(counts.true_positives);
        let false_negatives = counts.expected.saturating_sub(counts.true_positives);
        let precision = if counts.predicted > 0 {
            counts.true_positives as f32 / counts.predicted as f32
        } else {
            0.0
        };
        let recall = if counts.expected > 0 {
            counts.true_positives as f32 / counts.expected as f32
        } else {
            0.0
        };
        let f1 = harmonic_mean(precision, recall);

        metrics.push(EvalRuleMetrics {
            rule_id: rule_id.clone(),
            expected: counts.expected,
            predicted: counts.predicted,
            true_positives: counts.true_positives,
            false_positives,
            false_negatives,
            precision,
            recall,
            f1,
        });
    }

    metrics.sort_by(|left, right| {
        right
            .expected
            .cmp(&left.expected)
            .then_with(|| right.predicted.cmp(&left.predicted))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    metrics
}

fn summarize_rule_metrics(metrics: &[EvalRuleMetrics]) -> Option<EvalRuleScoreSummary> {
    if metrics.is_empty() {
        return None;
    }

    let mut tp_sum = 0usize;
    let mut predicted_sum = 0usize;
    let mut expected_sum = 0usize;
    let mut precision_sum = 0.0f32;
    let mut recall_sum = 0.0f32;
    let mut f1_sum = 0.0f32;

    for metric in metrics {
        tp_sum = tp_sum.saturating_add(metric.true_positives);
        predicted_sum = predicted_sum.saturating_add(metric.predicted);
        expected_sum = expected_sum.saturating_add(metric.expected);
        precision_sum += metric.precision;
        recall_sum += metric.recall;
        f1_sum += metric.f1;
    }

    let micro_precision = if predicted_sum > 0 {
        tp_sum as f32 / predicted_sum as f32
    } else {
        0.0
    };
    let micro_recall = if expected_sum > 0 {
        tp_sum as f32 / expected_sum as f32
    } else {
        0.0
    };
    let micro_f1 = harmonic_mean(micro_precision, micro_recall);
    let count = metrics.len() as f32;

    Some(EvalRuleScoreSummary {
        micro_precision,
        micro_recall,
        micro_f1,
        macro_precision: precision_sum / count,
        macro_recall: recall_sum / count,
        macro_f1: f1_sum / count,
    })
}

fn harmonic_mean(precision: f32, recall: f32) -> f32 {
    if precision + recall <= f32::EPSILON {
        0.0
    } else {
        (2.0 * precision * recall) / (precision + recall)
    }
}

impl EvalPattern {
    fn matches(&self, comment: &core::Comment) -> bool {
        if self.is_empty() {
            return false;
        }

        let content_lower = comment.content.to_ascii_lowercase();

        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                let candidate = comment.file_path.to_string_lossy();
                if !(candidate == file || candidate.ends_with(file)) {
                    return false;
                }
            }
        }

        if let Some(line) = self.line {
            if comment.line_number != line {
                return false;
            }
        }

        if let Some(contains) = &self.contains {
            let needle = contains.trim().to_ascii_lowercase();
            if !needle.is_empty() && !content_lower.contains(&needle) {
                return false;
            }
        }

        let contains_any: Vec<String> = self
            .contains_any
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect();
        if !contains_any.is_empty()
            && !contains_any
                .iter()
                .any(|needle| content_lower.contains(needle))
        {
            return false;
        }

        let tags_any: Vec<&str> = self
            .tags_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !tags_any.is_empty()
            && !tags_any.iter().any(|expected| {
                comment
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(expected))
            })
        {
            return false;
        }

        if let Some(pattern) = self.matches_regex.as_deref().map(str::trim) {
            if !pattern.is_empty()
                && !Regex::new(pattern)
                    .map(|regex| regex.is_match(&comment.content))
                    .unwrap_or(false)
            {
                return false;
            }
        }

        if let Some(severity) = &self.severity {
            if !comment
                .severity
                .to_string()
                .eq_ignore_ascii_case(severity.trim())
            {
                return false;
            }
        }

        if let Some(category) = &self.category {
            if !comment
                .category
                .to_string()
                .eq_ignore_ascii_case(category.trim())
            {
                return false;
            }
        }

        if let Some(min_confidence) = self.confidence_at_least {
            if comment.confidence < min_confidence {
                return false;
            }
        }

        if let Some(max_confidence) = self.confidence_at_most {
            if comment.confidence > max_confidence {
                return false;
            }
        }

        if let Some(fix_effort) = &self.fix_effort {
            let expected = fix_effort.trim();
            if !expected.is_empty()
                && !format!("{:?}", comment.fix_effort).eq_ignore_ascii_case(expected)
            {
                return false;
            }
        }

        if let Some(rule_id) = &self.rule_id {
            if self.require_rule_id {
                let expected = rule_id.trim().to_ascii_lowercase();
                let actual = comment
                    .rule_id
                    .as_deref()
                    .map(|value| value.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                if expected != actual {
                    return false;
                }
            }
        }

        true
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                parts.push(format!("file={}", file));
            }
        }
        if let Some(line) = self.line {
            parts.push(format!("line={}", line));
        }
        if let Some(contains) = &self.contains {
            let contains = contains.trim();
            if !contains.is_empty() {
                parts.push(format!("contains='{}'", contains));
            }
        }
        let contains_any: Vec<&str> = self
            .contains_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !contains_any.is_empty() {
            parts.push(format!("contains_any={}", contains_any.join("|")));
        }
        if let Some(pattern) = self.matches_regex.as_deref().map(str::trim) {
            if !pattern.is_empty() {
                parts.push(format!("matches_regex='{}'", pattern));
            }
        }
        if let Some(severity) = &self.severity {
            let severity = severity.trim();
            if !severity.is_empty() {
                parts.push(format!("severity={}", severity));
            }
        }
        if let Some(category) = &self.category {
            let category = category.trim();
            if !category.is_empty() {
                parts.push(format!("category={}", category));
            }
        }
        let tags_any: Vec<&str> = self
            .tags_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !tags_any.is_empty() {
            parts.push(format!("tags_any={}", tags_any.join("|")));
        }
        if let Some(min_confidence) = self.confidence_at_least {
            parts.push(format!("confidence>={:.2}", min_confidence));
        }
        if let Some(max_confidence) = self.confidence_at_most {
            parts.push(format!("confidence<={:.2}", max_confidence));
        }
        if let Some(fix_effort) = &self.fix_effort {
            let fix_effort = fix_effort.trim();
            if !fix_effort.is_empty() {
                parts.push(format!("fix_effort={}", fix_effort));
            }
        }
        if let Some(rule_id) = &self.rule_id {
            let rule_id = rule_id.trim();
            if !rule_id.is_empty() {
                if self.require_rule_id {
                    parts.push(format!("rule_id={} (required)", rule_id));
                } else {
                    parts.push(format!("rule_id={} (label)", rule_id));
                }
            }
        }

        if parts.is_empty() {
            "empty-pattern".to_string()
        } else {
            parts.join(", ")
        }
    }

    fn is_empty(&self) -> bool {
        self.file.as_deref().map(str::trim).unwrap_or("").is_empty()
            && self.line.is_none()
            && self
                .contains
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .contains_any
                .iter()
                .all(|value| value.trim().is_empty())
            && self
                .matches_regex
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .severity
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self
                .category
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && self.tags_any.iter().all(|value| value.trim().is_empty())
            && self.confidence_at_least.is_none()
            && self.confidence_at_most.is_none()
            && self
                .fix_effort
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            && (!self.require_rule_id
                || self
                    .rule_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty())
    }

    fn normalized_rule_id(&self) -> Option<String> {
        normalize_rule_id(self.rule_id.as_deref())
    }
}

fn summarize_for_eval(content: &str) -> String {
    let mut summary = content.trim().replace('\n', " ");
    if summary.len() > 120 {
        let mut end = 117;
        while end > 0 && !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
        summary.push_str("...");
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use crate::core::eval_benchmarks::{
        BenchmarkFixture, BenchmarkThresholds, CommunityFixturePack, Difficulty, ExpectedFinding,
        FixtureResult, NegativeFinding,
    };
    use tempfile::tempdir;

    #[test]
    fn test_summarize_for_eval_short() {
        let result = summarize_for_eval("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_summarize_for_eval_utf8_safety() {
        // Create a string with multi-byte UTF-8 chars that will land truncation mid-char
        // '€' is 3 bytes. 39 euros = 117 bytes exactly, but 38 euros = 114 bytes.
        // We need byte 117 to land mid-character.
        // 38 euros (114 bytes) + "abcd" (4 bytes) = 118 bytes > 120? No.
        // Let's use: 37 euros (111 bytes) + "abcdefghij" (10 bytes) = 121 bytes > 120
        // truncate(117): byte 117 = 111 + 6 = within "abcdefghij", which is ASCII. Safe.
        // Better: 39 euros (117 bytes) + "abcd" (4 bytes) = 121 bytes > 120
        // truncate(117): byte 117 is the end of the 39th euro. Safe boundary.
        // Better still: 38 euros (114 bytes) + "abc" (3 bytes) = 117. Not > 120.
        // We need: content where byte 117 is mid-char.
        // 39 euros = 117 bytes. Add "a" = 118 bytes. Not > 120.
        // 40 euros = 120 bytes. Add "a" = 121 bytes > 120.
        // truncate(117): byte 117 = end of 39th euro. 40th euro starts at 117.
        // Euro at bytes 117, 118, 119. truncate(117) is AT the start of the 40th euro.
        // is_char_boundary(117) — 117 is the start of a 3-byte char, so it IS a boundary!
        // Need byte 118: 40 euros (120 bytes) + "ab" = 122 bytes > 120.
        // Still truncate(117), which is start of 40th euro = valid boundary.
        // Use a mix: "a" + 39 euros = 1 + 117 = 118 bytes. Add "abc" = 121 > 120.
        // truncate(117): byte 117 = 1 + 38*3 = 115 is start of 39th euro.
        // byte 117 = 115 + 2 = mid-euro! This will panic!
        let content = format!("a{}{}", "€".repeat(39), "abc");
        // length = 1 + 117 + 3 = 121 bytes
        // truncate(117) = byte 117 = 1 + 38*3 + 2 = inside 39th euro
        let result = summarize_for_eval(&content);
        assert!(result.len() <= 120);
    }

    #[test]
    fn test_load_eval_fixtures_from_path_expands_benchmark_pack() {
        let dir = tempdir().unwrap();
        let pack_path = dir.path().join("pack.json");
        let pack = CommunityFixturePack {
            name: "owasp-top10".to_string(),
            author: "community".to_string(),
            version: "1.0.0".to_string(),
            description: "security regressions".to_string(),
            languages: vec!["python".to_string()],
            categories: vec!["security".to_string()],
            thresholds: Some(BenchmarkThresholds {
                min_precision: 0.8,
                min_recall: 0.7,
                min_f1: 0.75,
                max_false_positive_rate: 0.1,
                min_weighted_score: 0.77,
            }),
            metadata: HashMap::new(),
            fixtures: vec![BenchmarkFixture {
                name: "sql-injection".to_string(),
                category: "security".to_string(),
                language: "python".to_string(),
                difficulty: Difficulty::Easy,
                diff_content: "diff --git a/app.py b/app.py".to_string(),
                expected_findings: vec![ExpectedFinding {
                    description: "detect sql injection".to_string(),
                    severity: Some("error".to_string()),
                    category: Some("security".to_string()),
                    file_pattern: Some("app.py".to_string()),
                    line_hint: Some(12),
                    contains: Some("sql injection".to_string()),
                    rule_id: Some("sec.sql.injection".to_string()),
                }],
                negative_findings: vec![NegativeFinding {
                    description: "no false positive on sanitizer".to_string(),
                    file_pattern: Some("app.py".to_string()),
                    contains: Some("sanitized".to_string()),
                }],
                description: None,
                source: None,
            }],
        };
        std::fs::write(&pack_path, serde_json::to_string(&pack).unwrap()).unwrap();

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        let fixture = &fixtures[0];
        assert_eq!(
            fixture.fixture.name.as_deref(),
            Some("owasp-top10/sql-injection")
        );
        assert_eq!(fixture.suite_name.as_deref(), Some("owasp-top10"));
        assert_eq!(
            fixture.fixture.diff.as_deref(),
            Some("diff --git a/app.py b/app.py")
        );
        assert_eq!(fixture.fixture.expect.must_find.len(), 1);
        assert_eq!(fixture.fixture.expect.must_not_find.len(), 1);
        assert!(fixture.fixture.expect.must_find[0].require_rule_id);
        assert_eq!(fixture.difficulty.as_ref(), Some(&Difficulty::Easy));
        assert_eq!(
            fixture.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.sql.injection")
        );
        assert_eq!(
            fixture.suite_thresholds.as_ref().map(|value| value.min_f1),
            Some(0.75)
        );
    }

    #[test]
    fn test_load_eval_fixtures_from_path_keeps_standard_fixture_shape() {
        let dir = tempdir().unwrap();
        let fixture_path = dir.path().join("standard.yml");
        std::fs::write(
            &fixture_path,
            r#"name: standard
diff: |
  diff --git a/lib.rs b/lib.rs
expect:
  must_find:
    - contains: injection
      severity: error
"#,
        )
        .unwrap();

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(fixtures[0].fixture.name.as_deref(), Some("standard"));
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].contains.as_deref(),
            Some("injection")
        );
    }

    #[test]
    fn test_collect_eval_fixtures_expands_pack_entries_in_sorted_order() {
        let dir = tempdir().unwrap();
        let standard_path = dir.path().join("b-standard.yml");
        std::fs::write(
            &standard_path,
            r#"name: standard
diff: |
  diff --git a/lib.rs b/lib.rs
expect:
  must_find:
    - contains: unwrap
"#,
        )
        .unwrap();

        let pack_path = dir.path().join("a-pack.json");
        let pack = CommunityFixturePack {
            name: "community".to_string(),
            author: "tester".to_string(),
            version: "1.0.0".to_string(),
            description: "regressions".to_string(),
            languages: vec!["rust".to_string()],
            categories: vec!["correctness".to_string()],
            thresholds: None,
            metadata: HashMap::new(),
            fixtures: vec![BenchmarkFixture {
                name: "panic".to_string(),
                category: "correctness".to_string(),
                language: "rust".to_string(),
                difficulty: Difficulty::Medium,
                diff_content: "diff --git a/lib.rs b/lib.rs".to_string(),
                expected_findings: vec![],
                negative_findings: vec![],
                description: None,
                source: None,
            }],
        };
        std::fs::write(&pack_path, serde_json::to_string(&pack).unwrap()).unwrap();

        let fixtures = collect_eval_fixtures(dir.path()).unwrap();

        assert_eq!(fixtures.len(), 2);
        assert_eq!(fixtures[0].fixture.name.as_deref(), Some("community/panic"));
        assert_eq!(fixtures[1].fixture.name.as_deref(), Some("standard"));
    }

    #[test]
    fn test_evaluate_eval_thresholds_requires_baseline_for_drop_checks() {
        let report = EvalReport {
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![],
            rule_summary: Some(EvalRuleScoreSummary {
                micro_precision: 1.0,
                micro_recall: 1.0,
                micro_f1: 1.0,
                macro_precision: 1.0,
                macro_recall: 1.0,
                macro_f1: 1.0,
            }),
            suite_results: vec![],
            threshold_failures: vec![],
            results: vec![],
        };
        let options = EvalThresholdOptions {
            max_micro_f1_drop: Some(0.05),
            min_micro_f1: None,
            min_macro_f1: None,
            min_rule_f1: vec![],
            max_rule_f1_drop: vec![],
        };

        let failures = evaluate_eval_thresholds(&report, None, &options);

        assert_eq!(
            failures,
            vec!["baseline report is required for drop-based thresholds (--baseline)".to_string()]
        );
    }

    #[test]
    fn test_evaluate_eval_thresholds_checks_rule_specific_drop() {
        let current = EvalReport {
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: "sec.sql.injection".to_string(),
                expected: 1,
                predicted: 1,
                true_positives: 0,
                false_positives: 1,
                false_negatives: 1,
                precision: 0.0,
                recall: 0.0,
                f1: 0.0,
            }],
            rule_summary: Some(EvalRuleScoreSummary::default()),
            suite_results: vec![],
            threshold_failures: vec![],
            results: vec![],
        };
        let baseline = EvalReport {
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: "sec.sql.injection".to_string(),
                expected: 1,
                predicted: 1,
                true_positives: 1,
                false_positives: 0,
                false_negatives: 0,
                precision: 1.0,
                recall: 1.0,
                f1: 1.0,
            }],
            rule_summary: Some(EvalRuleScoreSummary::default()),
            suite_results: vec![],
            threshold_failures: vec![],
            results: vec![],
        };
        let options = EvalThresholdOptions {
            max_micro_f1_drop: None,
            min_micro_f1: None,
            min_macro_f1: None,
            min_rule_f1: vec![],
            max_rule_f1_drop: vec![EvalRuleThreshold {
                rule_id: "sec.sql.injection".to_string(),
                value: 0.2,
            }],
        };

        let failures = evaluate_eval_thresholds(&current, Some(&baseline), &options);

        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("sec.sql.injection"));
        assert!(failures[0].contains("exceeded max 0.200"));
    }

    #[test]
    fn test_eval_pattern_matches_regex_tags_and_confidence() {
        let comment = core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "Calling panic!(user_input) here can crash the request path".to_string(),
            rule_id: Some("panic.user-input".to_string()),
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: Some("Return an error instead of panicking".to_string()),
            confidence: 0.91,
            code_suggestion: None,
            tags: vec!["reliability".to_string(), "panic".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
        };

        let pattern = EvalPattern {
            contains_any: vec!["panic".to_string(), "unwrap".to_string()],
            matches_regex: Some("panic!\\([^)]*user_input[^)]*\\)".to_string()),
            tags_any: vec!["security".to_string(), "reliability".to_string()],
            confidence_at_least: Some(0.9),
            fix_effort: Some("low".to_string()),
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn test_build_suite_results_applies_pack_thresholds() {
        let results = vec![EvalFixtureResult {
            fixture: "community/sql-injection".to_string(),
            suite: Some("community".to_string()),
            passed: false,
            total_comments: 2,
            required_matches: 1,
            required_total: 1,
            benchmark_metrics: Some(FixtureResult {
                fixture_name: "community/sql-injection".to_string(),
                true_positives: 1,
                false_positives: 1,
                false_negatives: 0,
                true_negatives: 0,
                precision: 0.5,
                recall: 1.0,
                f1: 0.6666667,
                passed: false,
                details: vec![],
            }),
            suite_thresholds: Some(BenchmarkThresholds {
                min_precision: 0.9,
                min_recall: 0.9,
                min_f1: 0.9,
                max_false_positive_rate: 0.0,
                min_weighted_score: 0.95,
            }),
            difficulty: Some(Difficulty::Hard),
            rule_metrics: vec![],
            rule_summary: None,
            failures: vec!["missing finding".to_string()],
        }];

        let suites = build_suite_results(&results);

        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].suite, "community");
        assert!(!suites[0].threshold_pass);
        assert!(!suites[0].threshold_failures.is_empty());
    }
}
