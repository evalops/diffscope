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
        if !report.run.verification_judges.is_empty() {
            println!(
                "Verification judges: {}",
                report.run.verification_judges.join(", ")
            );
        }
        if let Some(consensus_mode) = report.run.verification_consensus_mode.as_deref() {
            println!("Verification consensus: {}", consensus_mode);
        }
        if let Some(trend_file) = report.run.trend_file.as_deref() {
            println!("Trend file: {}", trend_file);
        }
        if let Some(artifact_dir) = report.run.artifact_dir.as_deref() {
            println!("Artifact dir: {}", artifact_dir);
        }
        if let (Some(repeat_index), Some(repeat_total)) =
            (report.run.repeat_index, report.run.repeat_total)
        {
            println!("Repeat: {}/{}", repeat_index, repeat_total);
        }
        if report.run.reproduction_validation {
            println!("Reproduction validation: enabled");
        }
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
        if let Some(verification_report) = result.verification_report.as_ref() {
            println!(
                "  verification-consensus: {} (required_votes={} judges={})",
                verification_report.consensus_mode,
                verification_report.required_votes,
                verification_report.judge_count
            );
            for judge in &verification_report.judges {
                println!(
                    "    judge {}: passed={} filtered={} abstained={}",
                    judge.model,
                    judge.passed_comments,
                    judge.filtered_comments,
                    judge.abstained_comments
                );
            }
        }
        if let Some(agent_activity) = result.agent_activity.as_ref() {
            println!(
                "  review-agent: iterations={} tool-calls={}",
                agent_activity.total_iterations,
                agent_activity.tool_calls.len()
            );
        }
        if let Some(reproduction_summary) = result.reproduction_summary.as_ref() {
            println!(
                "  reproduction: confirmed={} rejected={} inconclusive={}",
                reproduction_summary.confirmed,
                reproduction_summary.rejected,
                reproduction_summary.inconclusive
            );
        }
        if let Some(artifact_path) = result.artifact_path.as_deref() {
            println!("  artifact: {}", artifact_path);
        }
        if !result.dag_traces.is_empty() {
            let traces = result
                .dag_traces
                .iter()
                .map(|trace| format!("{}({})", trace.graph_name, trace.records.len()))
                .collect::<Vec<_>>()
                .join(", ");
            println!("  dag: {}", traces);
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

    if let Some(benchmark_summary) = report.benchmark_summary.as_ref() {
        println!(
            "Benchmark summary: fixtures={} micro F1={:.0}% weighted={:.0}%",
            benchmark_summary.fixture_count,
            benchmark_summary.micro_f1 * 100.0,
            benchmark_summary.weighted_score * 100.0
        );
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

    if !report.suite_comparisons.is_empty() {
        println!("Baseline suite deltas:");
        for comparison in &report.suite_comparisons {
            println!(
                "  - {}: micro F1 {:+.0}% weighted {:+.0}% (baseline {:.0}% -> current {:.0}%)",
                comparison.name,
                comparison.micro_f1_delta * 100.0,
                comparison.weighted_score_delta * 100.0,
                comparison.baseline_micro_f1 * 100.0,
                comparison.current_micro_f1 * 100.0
            );
        }
    }

    if !report.category_comparisons.is_empty() {
        println!("Baseline category deltas:");
        for comparison in &report.category_comparisons {
            println!(
                "  - {}: micro F1 {:+.0}% weighted {:+.0}% (baseline {:.0}% -> current {:.0}%)",
                comparison.name,
                comparison.micro_f1_delta * 100.0,
                comparison.weighted_score_delta * 100.0,
                comparison.baseline_micro_f1 * 100.0,
                comparison.current_micro_f1 * 100.0
            );
        }
    }

    if !report.language_comparisons.is_empty() {
        println!("Baseline language deltas:");
        for comparison in &report.language_comparisons {
            println!(
                "  - {}: micro F1 {:+.0}% weighted {:+.0}% (baseline {:.0}% -> current {:.0}%)",
                comparison.name,
                comparison.micro_f1_delta * 100.0,
                comparison.weighted_score_delta * 100.0,
                comparison.baseline_micro_f1 * 100.0,
                comparison.current_micro_f1 * 100.0
            );
        }
    }

    if let Some(verification_health) = report.verification_health.as_ref() {
        println!(
            "Verification health: warnings={} fixtures={} fail-open={} parse-failures={} request-failures={}",
            verification_health.warnings_total,
            verification_health.fixtures_with_warnings,
            verification_health.fail_open_warning_count,
            verification_health.parse_failure_count,
            verification_health.request_failure_count
        );
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
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serialized).await?;
    Ok(())
}
