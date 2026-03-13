use super::super::metrics::{
    aggregate_rule_metrics, build_suite_results, collect_suite_threshold_failures,
    summarize_rule_metrics,
};
use super::super::thresholds::{evaluate_eval_thresholds, EvalThresholdOptions};
use super::super::{EvalFixtureResult, EvalReport};

pub(in super::super) fn build_eval_report(
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
