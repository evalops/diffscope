use anyhow::Result;
use std::path::Path;

use super::metrics::{
    aggregate_rule_metrics, build_suite_results, collect_suite_threshold_failures,
    summarize_rule_metrics,
};
use super::thresholds::{evaluate_eval_thresholds, EvalThresholdOptions};
use super::{EvalFixtureResult, EvalReport};

pub(super) fn build_eval_report(
    results: Vec<EvalFixtureResult>,
    baseline: Option<&EvalReport>,
    threshold_options: &EvalThresholdOptions,
) -> EvalReport {
    let fixtures_total = results.len();
    let fixtures_passed = results.iter().filter(|result| result.passed).count();
    let fixtures_failed = fixtures_total.saturating_sub(fixtures_passed);
    let rule_metrics = aggregate_rule_metrics(&results);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
    let suite_results = build_suite_results(&results);

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

    let mut threshold_failures = evaluate_eval_thresholds(&report, baseline, threshold_options);
    threshold_failures.extend(collect_suite_threshold_failures(&report.suite_results));
    report.threshold_failures = threshold_failures;
    report
}

pub(super) fn print_eval_report(report: &EvalReport) {
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

    for failure in &report.threshold_failures {
        println!("Threshold failure: {}", failure);
    }
}

pub(super) async fn write_eval_report(report: &EvalReport, path: &Path) -> Result<()> {
    let serialized = serde_json::to_string_pretty(report)?;
    tokio::fs::write(path, serialized).await?;
    Ok(())
}

pub(super) fn evaluation_failure_message(report: &EvalReport) -> Option<String> {
    if report.fixtures_failed == 0 && report.threshold_failures.is_empty() {
        return None;
    }

    let mut failure_parts = Vec::new();
    if report.fixtures_failed > 0 {
        failure_parts.push(format!(
            "{} fixture(s) did not meet expectations",
            report.fixtures_failed
        ));
    }
    if !report.threshold_failures.is_empty() {
        failure_parts.push(format!(
            "{} threshold check(s) failed",
            report.threshold_failures.len()
        ));
    }

    Some(format!("Evaluation failed: {}", failure_parts.join("; ")))
}
