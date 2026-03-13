use anyhow::Result;
use std::path::Path;

use super::super::EvalReport;

pub(in super::super) fn print_eval_report(report: &EvalReport) {
    if !report.run.model.is_empty() {
        println!(
            "Eval run: model={} provider={} selected={}/{} fixtures",
            report.run.model,
            report.run.provider.as_deref().unwrap_or("unknown"),
            report.run.fixtures_selected,
            report.run.fixtures_discovered
        );
        if let Some(label) = report.run.label.as_deref() {
            println!("Run label: {}", label);
        }
        if !report.run.started_at.is_empty() {
            println!("Started at: {}", report.run.started_at);
        }
        if !report.run.fixtures_root.is_empty() {
            println!("Fixtures root: {}", report.run.fixtures_root);
        }
        println!(
            "Verification fallback: {}",
            if report.run.verification_fail_open {
                "fail-open"
            } else {
                "strict"
            }
        );
    }

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
        if let Some(metadata) = result.metadata.as_ref() {
            let mut labels = Vec::new();
            if let Some(category) = metadata.category.as_deref() {
                labels.push(format!("category={}", category));
            }
            if let Some(language) = metadata.language.as_deref() {
                labels.push(format!("language={}", language));
            }
            if let Some(source) = metadata.source.as_deref() {
                labels.push(format!("source={}", source));
            }
            if !labels.is_empty() {
                println!("  metadata: {}", labels.join(", "));
            }
        }
        for warning in &result.warnings {
            println!("  warning: {}", warning);
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

    if !report.benchmark_by_category.is_empty() {
        println!("Benchmark categories:");
        let mut categories = report.benchmark_by_category.iter().collect::<Vec<_>>();
        categories.sort_by(|left, right| left.0.cmp(right.0));
        for (category, metrics) in categories {
            println!(
                "  - {}: fixtures={} micro F1={:.0}% weighted={:.0}%",
                category,
                metrics.fixture_count,
                metrics.micro_f1 * 100.0,
                metrics.weighted_score * 100.0
            );
        }
    }

    if !report.benchmark_by_language.is_empty() {
        println!("Benchmark languages:");
        let mut languages = report.benchmark_by_language.iter().collect::<Vec<_>>();
        languages.sort_by(|left, right| left.0.cmp(right.0));
        for (language, metrics) in languages {
            println!(
                "  - {}: fixtures={} micro F1={:.0}% weighted={:.0}%",
                language,
                metrics.fixture_count,
                metrics.micro_f1 * 100.0,
                metrics.weighted_score * 100.0
            );
        }
    }

    if !report.benchmark_by_difficulty.is_empty() {
        println!("Benchmark difficulties:");
        let mut difficulties = report.benchmark_by_difficulty.iter().collect::<Vec<_>>();
        difficulties.sort_by(|left, right| left.0.cmp(right.0));
        for (difficulty, metrics) in difficulties {
            println!(
                "  - {}: fixtures={} micro F1={:.0}% weighted={:.0}%",
                difficulty,
                metrics.fixture_count,
                metrics.micro_f1 * 100.0,
                metrics.weighted_score * 100.0
            );
        }
    }

    for warning in &report.warnings {
        println!("Warning: {}", warning);
    }

    for failure in &report.threshold_failures {
        println!("Threshold failure: {}", failure);
    }
}

pub(in super::super) async fn write_eval_report(report: &EvalReport, path: &Path) -> Result<()> {
    let serialized = serde_json::to_string_pretty(report)?;
    tokio::fs::write(path, serialized).await?;
    Ok(())
}
