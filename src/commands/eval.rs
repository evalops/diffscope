use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config;
use crate::core;
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
    severity: Option<String>,
    #[serde(default)]
    category: Option<String>,
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
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    total_comments: usize,
    #[serde(default)]
    required_matches: usize,
    #[serde(default)]
    required_total: usize,
    #[serde(default)]
    rule_metrics: Vec<EvalRuleMetrics>,
    #[serde(default)]
    rule_summary: Option<EvalRuleScoreSummary>,
    #[serde(default)]
    failures: Vec<String>,
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
    let fixture_paths = collect_fixture_paths(&fixtures_dir)?;
    if fixture_paths.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let mut results = Vec::new();
    for fixture_path in fixture_paths {
        let fixture = load_eval_fixture(&fixture_path)?;
        let result = run_eval_fixture(&config, &fixture_path, fixture).await?;
        results.push(result);
    }

    let fixtures_total = results.len();
    let fixtures_passed = results.iter().filter(|result| result.passed).count();
    let fixtures_failed = fixtures_total.saturating_sub(fixtures_passed);
    let rule_metrics = aggregate_rule_metrics(&results);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
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
        threshold_failures: Vec::new(),
        results,
    };
    let threshold_failures =
        evaluate_eval_thresholds(&report, baseline.as_ref(), &threshold_options);
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

fn load_eval_fixture(path: &Path) -> Result<EvalFixture> {
    let content = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Ok(serde_json::from_str(&content)?),
        _ => match serde_yaml::from_str(&content) {
            Ok(parsed) => Ok(parsed),
            Err(_) => Ok(serde_json::from_str(&content)?),
        },
    }
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

async fn run_eval_fixture(
    config: &config::Config,
    fixture_path: &Path,
    fixture: EvalFixture,
) -> Result<EvalFixtureResult> {
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

    let comments = review_diff_content_raw(&diff_content, config.clone(), &repo_path).await?;
    let total_comments = comments.len();
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let required_total = fixture.expect.must_find.len();
    let mut used_comment_indices = HashSet::new();
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
        if let Some(comment) = comments.iter().find(|comment| unexpected.matches(comment)) {
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

    Ok(EvalFixtureResult {
        fixture: fixture_name,
        passed: failures.is_empty(),
        total_comments,
        required_matches,
        required_total,
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
            if !needle.is_empty() && !comment.content.to_ascii_lowercase().contains(&needle) {
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
}
