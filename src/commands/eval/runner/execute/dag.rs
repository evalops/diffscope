use anyhow::Result;
use std::collections::HashSet;
use std::time::Instant;

use crate::config;
use crate::core;
use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;
use crate::review::review_diff_content_raw;

use super::super::super::{EvalAgentActivity, EvalReproductionSummary, EvalVerificationReport};
use super::super::matching::{evaluate_fixture_expectations, FixtureMatchSummary};
use super::artifact::{
    maybe_write_fixture_artifact, EvalFixtureArtifactContext, EvalFixtureArtifactInput,
};
use super::loading::PreparedFixtureExecution;
use super::repro::maybe_run_reproduction_validation;
use super::result::{
    append_total_comment_failures, build_benchmark_metrics, convert_agent_activity,
    convert_verification_report, FixtureResultDetails,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EvalFixtureStage {
    Review,
    ExpectationMatching,
    ReproductionValidation,
    ArtifactCapture,
}

#[cfg(test)]
impl EvalFixtureStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::ExpectationMatching => "expectation_matching",
            Self::ReproductionValidation => "reproduction_validation",
            Self::ArtifactCapture => "artifact_capture",
        }
    }
}

#[derive(Debug, Clone)]
struct EvalFixtureStageSpec {
    stage: EvalFixtureStage,
    dependencies: Vec<EvalFixtureStage>,
    enabled: bool,
}

pub(super) struct EvalFixtureDagConfig<'a> {
    pub(super) repro_validate: bool,
    pub(super) repro_max_comments: usize,
    pub(super) artifact_context: Option<&'a EvalFixtureArtifactContext>,
}

pub(super) struct EvalFixtureExecutionOutcome {
    pub(super) prepared: PreparedFixtureExecution,
    pub(super) total_comments: usize,
    pub(super) match_summary: FixtureMatchSummary,
    pub(super) benchmark_metrics: Option<BenchmarkFixtureResult>,
    pub(super) details: FixtureResultDetails,
}

struct EvalFixtureDagContext<'a> {
    prepared: PreparedFixtureExecution,
    dag_config: EvalFixtureDagConfig<'a>,
    comments: Vec<core::Comment>,
    warnings: Vec<String>,
    verification_report: Option<EvalVerificationReport>,
    agent_activity: Option<EvalAgentActivity>,
    reproduction_summary: Option<EvalReproductionSummary>,
    total_comments: usize,
    match_summary: Option<FixtureMatchSummary>,
    failures: Vec<String>,
    benchmark_metrics: Option<BenchmarkFixtureResult>,
    artifact_path: Option<String>,
}

impl<'a> EvalFixtureDagContext<'a> {
    fn new(prepared: PreparedFixtureExecution, dag_config: EvalFixtureDagConfig<'a>) -> Self {
        Self {
            prepared,
            dag_config,
            comments: Vec::new(),
            warnings: Vec::new(),
            verification_report: None,
            agent_activity: None,
            reproduction_summary: None,
            total_comments: 0,
            match_summary: None,
            failures: Vec::new(),
            benchmark_metrics: None,
            artifact_path: None,
        }
    }

    fn into_outcome(self) -> Result<EvalFixtureExecutionOutcome> {
        Ok(EvalFixtureExecutionOutcome {
            prepared: self.prepared,
            total_comments: self.total_comments,
            match_summary: self.match_summary.ok_or_else(|| {
                anyhow::anyhow!("fixture DAG did not produce expectation matches")
            })?,
            benchmark_metrics: self.benchmark_metrics,
            details: FixtureResultDetails {
                warnings: self.warnings,
                verification_report: self.verification_report,
                agent_activity: self.agent_activity,
                reproduction_summary: self.reproduction_summary,
                artifact_path: self.artifact_path,
                failures: self.failures,
            },
        })
    }
}

pub(super) async fn execute_eval_fixture_dag(
    config: &config::Config,
    prepared: PreparedFixtureExecution,
    dag_config: EvalFixtureDagConfig<'_>,
) -> Result<EvalFixtureExecutionOutcome> {
    let specs = build_stage_specs(dag_config.repro_validate);
    let mut context = EvalFixtureDagContext::new(prepared, dag_config);
    let mut completed = HashSet::new();

    while completed.len() < specs.len() {
        let Some(spec) = specs
            .iter()
            .find(|candidate| {
                !completed.contains(&candidate.stage)
                    && candidate
                        .dependencies
                        .iter()
                        .all(|dependency| completed.contains(dependency))
            })
            .cloned()
        else {
            anyhow::bail!("eval fixture DAG has unresolved stage dependencies");
        };

        let _started = Instant::now();
        if spec.enabled {
            execute_stage(spec.stage, config, &mut context).await?;
        }
        completed.insert(spec.stage);
    }

    context.into_outcome()
}

fn build_stage_specs(repro_validate: bool) -> Vec<EvalFixtureStageSpec> {
    vec![
        EvalFixtureStageSpec {
            stage: EvalFixtureStage::Review,
            dependencies: vec![],
            enabled: true,
        },
        EvalFixtureStageSpec {
            stage: EvalFixtureStage::ExpectationMatching,
            dependencies: vec![EvalFixtureStage::Review],
            enabled: true,
        },
        EvalFixtureStageSpec {
            stage: EvalFixtureStage::ReproductionValidation,
            dependencies: vec![EvalFixtureStage::Review],
            enabled: repro_validate,
        },
        EvalFixtureStageSpec {
            stage: EvalFixtureStage::ArtifactCapture,
            dependencies: if repro_validate {
                vec![
                    EvalFixtureStage::ExpectationMatching,
                    EvalFixtureStage::ReproductionValidation,
                ]
            } else {
                vec![EvalFixtureStage::ExpectationMatching]
            },
            enabled: true,
        },
    ]
}

async fn execute_stage(
    stage: EvalFixtureStage,
    config: &config::Config,
    context: &mut EvalFixtureDagContext<'_>,
) -> Result<()> {
    match stage {
        EvalFixtureStage::Review => execute_review_stage(config, context).await,
        EvalFixtureStage::ExpectationMatching => execute_expectation_stage(context),
        EvalFixtureStage::ReproductionValidation => {
            execute_reproduction_stage(config, context).await
        }
        EvalFixtureStage::ArtifactCapture => execute_artifact_stage(context).await,
    }
}

async fn execute_review_stage(
    config: &config::Config,
    context: &mut EvalFixtureDagContext<'_>,
) -> Result<()> {
    let review_result = review_diff_content_raw(
        &context.prepared.diff_content,
        config.clone(),
        &context.prepared.repo_path,
    )
    .await?;
    context.verification_report = convert_verification_report(review_result.verification_report);
    context.agent_activity = convert_agent_activity(review_result.agent_activity);
    context.comments = review_result.comments;
    context.warnings = review_result.warnings;
    context.total_comments = context.comments.len();
    Ok(())
}

fn execute_expectation_stage(context: &mut EvalFixtureDagContext<'_>) -> Result<()> {
    let match_summary =
        evaluate_fixture_expectations(&context.prepared.fixture.expect, &context.comments);
    let mut failures = match_summary.failures.clone();
    append_total_comment_failures(
        &mut failures,
        context.total_comments,
        &context.prepared.fixture.expect,
    );
    context.benchmark_metrics = build_benchmark_metrics(
        &context.prepared,
        context.total_comments,
        &match_summary,
        &failures,
    );
    context.match_summary = Some(match_summary);
    context.failures = failures;
    Ok(())
}

async fn execute_reproduction_stage(
    config: &config::Config,
    context: &mut EvalFixtureDagContext<'_>,
) -> Result<()> {
    context.reproduction_summary = maybe_run_reproduction_validation(
        config,
        &context.prepared,
        &context.comments,
        context.dag_config.repro_max_comments,
    )
    .await?;
    if let Some(summary) = context.reproduction_summary.as_ref() {
        context
            .warnings
            .extend(summary.checks.iter().filter_map(|check| {
                check.warning.as_ref().map(|warning| {
                    format!(
                        "reproduction validator for comment {} ({}) reported: {}",
                        check.comment_id, check.model, warning
                    )
                })
            }));
    }
    Ok(())
}

async fn execute_artifact_stage(context: &mut EvalFixtureDagContext<'_>) -> Result<()> {
    let Some(match_summary) = context.match_summary.as_ref() else {
        anyhow::bail!("artifact stage requires expectation matching output");
    };
    context.artifact_path = maybe_write_fixture_artifact(EvalFixtureArtifactInput {
        context: context.dag_config.artifact_context,
        prepared: &context.prepared,
        total_comments: context.total_comments,
        comments: &context.comments,
        warnings: &context.warnings,
        failures: &context.failures,
        benchmark_metrics: context.benchmark_metrics.as_ref(),
        rule_metrics: &match_summary.rule_metrics,
        rule_summary: match_summary.rule_summary,
        verification_report: context.verification_report.as_ref(),
        agent_activity: context.agent_activity.as_ref(),
        reproduction_summary: context.reproduction_summary.as_ref(),
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_stage_specs_links_artifact_to_reproduction_when_enabled() {
        let specs = build_stage_specs(true);
        let artifact = specs
            .iter()
            .find(|spec| spec.stage == EvalFixtureStage::ArtifactCapture)
            .unwrap();

        assert!(artifact
            .dependencies
            .contains(&EvalFixtureStage::ExpectationMatching));
        assert!(artifact
            .dependencies
            .contains(&EvalFixtureStage::ReproductionValidation));
    }

    #[test]
    fn build_stage_specs_keeps_reproduction_optional() {
        let specs = build_stage_specs(false);
        let reproduction = specs
            .iter()
            .find(|spec| spec.stage == EvalFixtureStage::ReproductionValidation)
            .unwrap();
        let artifact = specs
            .iter()
            .find(|spec| spec.stage == EvalFixtureStage::ArtifactCapture)
            .unwrap();

        assert!(!reproduction.enabled);
        assert_eq!(
            artifact
                .dependencies
                .iter()
                .map(|dependency| dependency.as_str())
                .collect::<Vec<_>>(),
            vec!["expectation_matching"]
        );
    }
}
