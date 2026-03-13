#[path = "execute/artifact.rs"]
mod artifact;
#[path = "execute/loading.rs"]
mod loading;
#[path = "execute/result.rs"]
mod result;

use anyhow::Result;

use crate::config;
use crate::review::review_diff_content_raw;

use self::artifact::{maybe_write_fixture_artifact, EvalFixtureArtifactInput};
use self::loading::prepare_fixture_execution;
use self::result::{append_total_comment_failures, build_benchmark_metrics, build_fixture_result};
use super::super::{EvalFixtureResult, LoadedEvalFixture};
use super::matching::evaluate_fixture_expectations;

pub(in super::super) use self::artifact::EvalFixtureArtifactContext;

pub(in super::super) async fn run_eval_fixture(
    config: &config::Config,
    loaded_fixture: LoadedEvalFixture,
    artifact_context: Option<&EvalFixtureArtifactContext>,
) -> Result<EvalFixtureResult> {
    let prepared = prepare_fixture_execution(loaded_fixture)?;
    let review_result =
        review_diff_content_raw(&prepared.diff_content, config.clone(), &prepared.repo_path)
            .await?;
    let warnings = review_result.warnings;
    let comments = review_result.comments;
    let total_comments = comments.len();
    let match_summary = evaluate_fixture_expectations(&prepared.fixture.expect, &comments);
    let mut failures = match_summary.failures.clone();

    append_total_comment_failures(&mut failures, total_comments, &prepared.fixture.expect);
    let benchmark_metrics =
        build_benchmark_metrics(&prepared, total_comments, &match_summary, &failures);
    let artifact_path = maybe_write_fixture_artifact(EvalFixtureArtifactInput {
        context: artifact_context,
        prepared: &prepared,
        total_comments,
        comments: &comments,
        warnings: &warnings,
        failures: &failures,
        benchmark_metrics: benchmark_metrics.as_ref(),
        rule_metrics: &match_summary.rule_metrics,
        rule_summary: match_summary.rule_summary,
    })
    .await?;

    Ok(build_fixture_result(
        prepared,
        total_comments,
        match_summary,
        benchmark_metrics,
        warnings,
        artifact_path,
        failures,
    ))
}
