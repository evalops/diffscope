use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;

use super::super::super::{EvalExpectations, EvalFixtureResult};
use super::super::matching::FixtureMatchSummary;
use super::loading::PreparedFixtureExecution;

pub(super) fn append_total_comment_failures(
    failures: &mut Vec<String>,
    total_comments: usize,
    expectations: &EvalExpectations,
) {
    if let Some(min_total) = expectations.min_total {
        if total_comments < min_total {
            failures.push(format!(
                "Expected at least {} comments, got {}",
                min_total, total_comments
            ));
        }
    }
    if let Some(max_total) = expectations.max_total {
        if total_comments > max_total {
            failures.push(format!(
                "Expected at most {} comments, got {}",
                max_total, total_comments
            ));
        }
    }
}

pub(super) fn build_benchmark_metrics(
    prepared: &PreparedFixtureExecution,
    total_comments: usize,
    match_summary: &FixtureMatchSummary,
    failures: &[String],
) -> Option<BenchmarkFixtureResult> {
    prepared.suite_name.as_ref().map(|_| {
        let accounted_for = match_summary
            .used_comment_indices
            .union(&match_summary.unexpected_comment_indices)
            .count();
        let extra_findings = total_comments.saturating_sub(accounted_for);
        let mut result = BenchmarkFixtureResult::compute(
            &prepared.fixture_name,
            prepared.fixture.expect.must_find.len(),
            prepared.fixture.expect.must_not_find.len(),
            match_summary.required_matches,
            match_summary.unexpected_comment_indices.len(),
            extra_findings,
        );
        result.details = failures.to_vec();
        result
    })
}

pub(super) fn build_fixture_result(
    prepared: PreparedFixtureExecution,
    total_comments: usize,
    match_summary: FixtureMatchSummary,
    benchmark_metrics: Option<BenchmarkFixtureResult>,
    warnings: Vec<String>,
    artifact_path: Option<String>,
    failures: Vec<String>,
) -> EvalFixtureResult {
    EvalFixtureResult {
        fixture: prepared.fixture_name,
        suite: prepared.suite_name,
        passed: failures.is_empty(),
        total_comments,
        required_matches: match_summary.required_matches,
        required_total: match_summary.required_total,
        benchmark_metrics,
        suite_thresholds: prepared.suite_thresholds,
        difficulty: prepared.difficulty,
        metadata: prepared.metadata,
        rule_metrics: match_summary.rule_metrics,
        rule_summary: match_summary.rule_summary,
        warnings,
        artifact_path,
        failures,
    }
}
