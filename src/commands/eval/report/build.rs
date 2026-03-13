use super::super::metrics::{
    aggregate_rule_metrics, build_benchmark_breakdowns, build_named_breakdown_comparisons,
    build_overall_benchmark_summary, build_suite_comparisons, build_suite_results,
    build_verification_health, collect_suite_threshold_failures, summarize_rule_metrics,
};
use super::super::thresholds::{evaluate_eval_thresholds, EvalThresholdOptions};
use super::super::{EvalFixtureResult, EvalReport, EvalRunMetadata};

pub(in super::super) fn build_eval_report(
    results: Vec<EvalFixtureResult>,
    baseline: Option<&EvalReport>,
    threshold_options: &EvalThresholdOptions,
    run: EvalRunMetadata,
) -> EvalReport {
    let fixtures_total = results.len();
    let fixtures_passed = results.iter().filter(|result| result.passed).count();
    let fixtures_failed = fixtures_total.saturating_sub(fixtures_passed);
    let warnings = results
        .iter()
        .flat_map(|result| {
            result
                .warnings
                .iter()
                .map(|warning| format!("{}: {}", result.fixture, warning))
        })
        .collect::<Vec<_>>();
    let rule_metrics = aggregate_rule_metrics(&results);
    let rule_summary = summarize_rule_metrics(&rule_metrics);
    let benchmark_summary = build_overall_benchmark_summary(&results);
    let suite_results = build_suite_results(&results);
    let breakdowns = build_benchmark_breakdowns(&results);
    let suite_comparisons = build_suite_comparisons(&suite_results, baseline);
    let category_comparisons = build_named_breakdown_comparisons(
        &breakdowns.by_category,
        baseline.map(|report| &report.benchmark_by_category),
    );
    let language_comparisons = build_named_breakdown_comparisons(
        &breakdowns.by_language,
        baseline.map(|report| &report.benchmark_by_language),
    );
    let verification_health = build_verification_health(&results);

    let mut report = EvalReport {
        run,
        fixtures_total,
        fixtures_passed,
        fixtures_failed,
        rule_metrics,
        rule_summary,
        benchmark_summary,
        suite_results,
        benchmark_by_category: breakdowns.by_category,
        benchmark_by_language: breakdowns.by_language,
        benchmark_by_difficulty: breakdowns.by_difficulty,
        suite_comparisons,
        category_comparisons,
        language_comparisons,
        verification_health,
        warnings,
        threshold_failures: Vec::new(),
        results,
    };

    let mut threshold_failures = evaluate_eval_thresholds(&report, baseline, threshold_options);
    threshold_failures.extend(collect_suite_threshold_failures(&report.suite_results));
    report.threshold_failures = threshold_failures;
    report
}
