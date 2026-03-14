use crate::core::dag::DagExecutionTrace;
use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;

use super::super::super::{
    EvalAgentActivity, EvalAgentToolCall, EvalExpectations, EvalFixtureResult,
    EvalReproductionSummary, EvalVerificationJudgeReport, EvalVerificationReport,
};
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
    details: FixtureResultDetails,
) -> EvalFixtureResult {
    EvalFixtureResult {
        fixture: prepared.fixture_name,
        suite: prepared.suite_name,
        passed: details.failures.is_empty(),
        total_comments,
        required_matches: match_summary.required_matches,
        required_total: match_summary.required_total,
        benchmark_metrics,
        suite_thresholds: prepared.suite_thresholds,
        difficulty: prepared.difficulty,
        metadata: prepared.metadata,
        rule_metrics: match_summary.rule_metrics,
        rule_summary: match_summary.rule_summary,
        warnings: details.warnings,
        verification_report: details.verification_report,
        agent_activity: details.agent_activity,
        reproduction_summary: details.reproduction_summary,
        artifact_path: details.artifact_path,
        failures: details.failures,
        dag_traces: details.dag_traces,
    }
}

pub(super) struct FixtureResultDetails {
    pub(super) warnings: Vec<String>,
    pub(super) verification_report: Option<EvalVerificationReport>,
    pub(super) agent_activity: Option<EvalAgentActivity>,
    pub(super) reproduction_summary: Option<EvalReproductionSummary>,
    pub(super) artifact_path: Option<String>,
    pub(super) failures: Vec<String>,
    pub(super) dag_traces: Vec<DagExecutionTrace>,
}

pub(super) fn convert_agent_activity(
    activity: Option<crate::review::AgentActivity>,
) -> Option<EvalAgentActivity> {
    activity.map(|activity| EvalAgentActivity {
        total_iterations: activity.total_iterations,
        tool_calls: activity
            .tool_calls
            .into_iter()
            .map(|call| EvalAgentToolCall {
                iteration: call.iteration,
                tool_name: call.tool_name,
                duration_ms: call.duration_ms,
            })
            .collect(),
    })
}

pub(super) fn convert_verification_report(
    report: Option<crate::review::verification::VerificationReport>,
) -> Option<EvalVerificationReport> {
    report.map(|report| EvalVerificationReport {
        consensus_mode: report.consensus_mode,
        required_votes: report.required_votes,
        judge_count: report.judge_count,
        judges: report
            .judges
            .into_iter()
            .map(|judge| EvalVerificationJudgeReport {
                model: judge.model,
                total_comments: judge.total_comments,
                passed_comments: judge.passed_comments,
                filtered_comments: judge.filtered_comments,
                abstained_comments: judge.abstained_comments,
                warnings: judge.warnings,
            })
            .collect(),
    })
}
