#[path = "execute/artifact.rs"]
mod artifact;
#[path = "execute/dag.rs"]
mod dag;
#[path = "execute/loading.rs"]
mod loading;
#[path = "execute/repro.rs"]
mod repro;
#[path = "execute/result.rs"]
mod result;

use anyhow::Result;

use crate::config;

use self::dag::{execute_eval_fixture_dag, EvalFixtureDagConfig};
use self::loading::prepare_fixture_execution;
use self::result::build_fixture_result;
use super::super::{EvalFixtureResult, LoadedEvalFixture};

pub(in super::super) use self::artifact::EvalFixtureArtifactContext;

pub(crate) fn describe_eval_fixture_graph(
    repro_validate: bool,
) -> crate::core::dag::DagGraphContract {
    self::dag::describe_eval_fixture_graph(repro_validate)
}

pub(in super::super) async fn run_eval_fixture(
    config: &config::Config,
    loaded_fixture: LoadedEvalFixture,
    repro_validate: bool,
    repro_max_comments: usize,
    artifact_context: Option<&EvalFixtureArtifactContext>,
) -> Result<EvalFixtureResult> {
    let prepared = prepare_fixture_execution(loaded_fixture)?;
    let outcome = execute_eval_fixture_dag(
        config,
        prepared,
        EvalFixtureDagConfig {
            repro_validate,
            repro_max_comments,
            artifact_context: artifact_context.cloned(),
        },
    )
    .await?;

    Ok(build_fixture_result(
        outcome.prepared,
        outcome.total_comments,
        outcome.match_summary,
        outcome.benchmark_metrics,
        outcome.details,
    ))
}
