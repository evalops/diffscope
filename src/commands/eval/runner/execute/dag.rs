use anyhow::Result;
use futures::FutureExt;
use tracing::debug;

use crate::config;
use crate::core;
use crate::core::dag::{
    describe_dag, execute_dag, DagExecutionRecord, DagExecutionTrace, DagGraphContract, DagNode,
    DagNodeContract, DagNodeExecutionHints, DagNodeKind, DagNodeSpec,
};
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
    CommentCountValidation,
    BenchmarkMetrics,
    ReproductionValidation,
    ArtifactCapture,
}

impl DagNode for EvalFixtureStage {
    fn name(&self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::ExpectationMatching => "expectation_matching",
            Self::CommentCountValidation => "comment_count_validation",
            Self::BenchmarkMetrics => "benchmark_metrics",
            Self::ReproductionValidation => "reproduction_validation",
            Self::ArtifactCapture => "artifact_capture",
        }
    }
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
    dag_traces: Vec<DagExecutionTrace>,
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
            dag_traces: Vec::new(),
        }
    }

    fn into_outcome(
        self,
        eval_records: Vec<DagExecutionRecord>,
    ) -> Result<EvalFixtureExecutionOutcome> {
        let mut dag_traces = self.dag_traces;
        dag_traces.push(DagExecutionTrace {
            graph_name: "eval_fixture_execution".to_string(),
            records: eval_records,
        });
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
                dag_traces,
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
    let dag_description = describe_dag(&specs);
    debug!(?dag_description, "Executing eval fixture DAG");
    let mut context = EvalFixtureDagContext::new(prepared, dag_config);
    let records = execute_dag(&specs, &mut context, |stage, context| {
        async move { execute_stage(stage, config, context).await }.boxed()
    })
    .await?;
    rewrite_fixture_artifact_with_eval_trace(&mut context, &records).await?;

    context.into_outcome(records)
}

pub(in super::super::super) fn describe_eval_fixture_graph(
    repro_validate: bool,
) -> DagGraphContract {
    let nodes = build_stage_specs(repro_validate)
        .into_iter()
        .map(|spec| match spec.id {
            EvalFixtureStage::Review => DagNodeContract {
                name: spec.id.name().to_string(),
                description:
                    "Run the review pipeline over fixture diff content and collect raw comments."
                        .to_string(),
                kind: DagNodeKind::Execution,
                dependencies: vec![],
                inputs: vec![
                    "config".to_string(),
                    "prepared_fixture".to_string(),
                    "repo_path".to_string(),
                ],
                outputs: vec![
                    "comments".to_string(),
                    "warnings".to_string(),
                    "verification_report".to_string(),
                    "agent_activity".to_string(),
                ],
                hints: DagNodeExecutionHints {
                    parallelizable: false,
                    retryable: true,
                    side_effects: false,
                    subgraph: Some("review_pipeline".to_string()),
                },
                enabled: spec.enabled,
            },
            EvalFixtureStage::ExpectationMatching => DagNodeContract {
                name: spec.id.name().to_string(),
                description:
                    "Match emitted comments against expected and negative fixture findings."
                        .to_string(),
                kind: DagNodeKind::Validation,
                dependencies: spec
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name().to_string())
                    .collect(),
                inputs: vec!["comments".to_string(), "fixture_expectations".to_string()],
                outputs: vec!["match_summary".to_string(), "failures".to_string()],
                hints: DagNodeExecutionHints {
                    parallelizable: true,
                    retryable: true,
                    side_effects: false,
                    subgraph: None,
                },
                enabled: spec.enabled,
            },
            EvalFixtureStage::CommentCountValidation => DagNodeContract {
                name: spec.id.name().to_string(),
                description: "Check fixture-level minimum and maximum comment count expectations."
                    .to_string(),
                kind: DagNodeKind::Validation,
                dependencies: spec
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name().to_string())
                    .collect(),
                inputs: vec![
                    "total_comments".to_string(),
                    "fixture_expectations".to_string(),
                    "failures".to_string(),
                ],
                outputs: vec!["failures".to_string()],
                hints: DagNodeExecutionHints {
                    parallelizable: true,
                    retryable: true,
                    side_effects: false,
                    subgraph: None,
                },
                enabled: spec.enabled,
            },
            EvalFixtureStage::BenchmarkMetrics => DagNodeContract {
                name: spec.id.name().to_string(),
                description: "Build benchmark metrics and pass/fail signals from match outcomes."
                    .to_string(),
                kind: DagNodeKind::Analysis,
                dependencies: spec
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name().to_string())
                    .collect(),
                inputs: vec![
                    "prepared_fixture".to_string(),
                    "total_comments".to_string(),
                    "match_summary".to_string(),
                    "failures".to_string(),
                ],
                outputs: vec!["benchmark_metrics".to_string()],
                hints: DagNodeExecutionHints {
                    parallelizable: true,
                    retryable: true,
                    side_effects: false,
                    subgraph: None,
                },
                enabled: spec.enabled,
            },
            EvalFixtureStage::ReproductionValidation => DagNodeContract {
                name: spec.id.name().to_string(),
                description:
                    "Use bounded tool-backed reproduction checks to validate selected comments."
                        .to_string(),
                kind: DagNodeKind::Validation,
                dependencies: spec
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name().to_string())
                    .collect(),
                inputs: vec![
                    "config".to_string(),
                    "prepared_fixture".to_string(),
                    "comments".to_string(),
                ],
                outputs: vec!["reproduction_summary".to_string(), "warnings".to_string()],
                hints: DagNodeExecutionHints {
                    parallelizable: false,
                    retryable: true,
                    side_effects: false,
                    subgraph: None,
                },
                enabled: spec.enabled,
            },
            EvalFixtureStage::ArtifactCapture => DagNodeContract {
                name: spec.id.name().to_string(),
                description:
                    "Persist fixture-level artifacts for debugging and offline inspection."
                        .to_string(),
                kind: DagNodeKind::Persistence,
                dependencies: spec
                    .dependencies
                    .into_iter()
                    .map(|dependency| dependency.name().to_string())
                    .collect(),
                inputs: vec![
                    "prepared_fixture".to_string(),
                    "comments".to_string(),
                    "warnings".to_string(),
                    "failures".to_string(),
                    "benchmark_metrics".to_string(),
                ],
                outputs: vec!["artifact_path".to_string()],
                hints: DagNodeExecutionHints {
                    parallelizable: false,
                    retryable: true,
                    side_effects: true,
                    subgraph: None,
                },
                enabled: spec.enabled,
            },
        })
        .collect::<Vec<_>>();

    DagGraphContract {
        name: "eval_fixture_execution".to_string(),
        description:
            "Fixture-scoped evaluation DAG for review, matching, scoring, reproduction, and artifact capture."
                .to_string(),
        entry_nodes: vec!["review".to_string()],
        terminal_nodes: vec!["artifact_capture".to_string()],
        nodes,
    }
}

fn build_stage_specs(repro_validate: bool) -> Vec<DagNodeSpec<EvalFixtureStage>> {
    vec![
        DagNodeSpec {
            id: EvalFixtureStage::Review,
            dependencies: vec![],
            enabled: true,
        },
        DagNodeSpec {
            id: EvalFixtureStage::ExpectationMatching,
            dependencies: vec![EvalFixtureStage::Review],
            enabled: true,
        },
        DagNodeSpec {
            id: EvalFixtureStage::CommentCountValidation,
            dependencies: vec![EvalFixtureStage::ExpectationMatching],
            enabled: true,
        },
        DagNodeSpec {
            id: EvalFixtureStage::BenchmarkMetrics,
            dependencies: vec![EvalFixtureStage::CommentCountValidation],
            enabled: true,
        },
        DagNodeSpec {
            id: EvalFixtureStage::ReproductionValidation,
            dependencies: vec![EvalFixtureStage::Review],
            enabled: repro_validate,
        },
        DagNodeSpec {
            id: EvalFixtureStage::ArtifactCapture,
            dependencies: if repro_validate {
                vec![
                    EvalFixtureStage::BenchmarkMetrics,
                    EvalFixtureStage::ReproductionValidation,
                ]
            } else {
                vec![EvalFixtureStage::BenchmarkMetrics]
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
        EvalFixtureStage::CommentCountValidation => execute_comment_count_stage(context),
        EvalFixtureStage::BenchmarkMetrics => execute_benchmark_metrics_stage(context),
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
    context.dag_traces = review_result.dag_traces;
    context.comments = review_result.comments;
    context.warnings = review_result.warnings;
    context.total_comments = context.comments.len();
    Ok(())
}

fn execute_expectation_stage(context: &mut EvalFixtureDagContext<'_>) -> Result<()> {
    let match_summary =
        evaluate_fixture_expectations(&context.prepared.fixture.expect, &context.comments);
    context.failures = match_summary.failures.clone();
    context.match_summary = Some(match_summary);
    Ok(())
}

fn execute_comment_count_stage(context: &mut EvalFixtureDagContext<'_>) -> Result<()> {
    if context.match_summary.is_none() {
        anyhow::bail!("comment count validation requires expectation matches");
    }
    append_total_comment_failures(
        &mut context.failures,
        context.total_comments,
        &context.prepared.fixture.expect,
    );
    Ok(())
}

fn execute_benchmark_metrics_stage(context: &mut EvalFixtureDagContext<'_>) -> Result<()> {
    let Some(match_summary) = context.match_summary.as_ref() else {
        anyhow::bail!("benchmark metrics require expectation matches");
    };
    context.benchmark_metrics = build_benchmark_metrics(
        &context.prepared,
        context.total_comments,
        match_summary,
        &context.failures,
    );
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
        dag_traces: &context.dag_traces,
    })
    .await?;
    Ok(())
}

async fn rewrite_fixture_artifact_with_eval_trace(
    context: &mut EvalFixtureDagContext<'_>,
    eval_records: &[DagExecutionRecord],
) -> Result<()> {
    if context.artifact_path.is_none() {
        return Ok(());
    }
    let Some(match_summary) = context.match_summary.as_ref() else {
        anyhow::bail!("artifact rewrite requires expectation matching output");
    };

    let mut dag_traces = context.dag_traces.clone();
    dag_traces.push(DagExecutionTrace {
        graph_name: "eval_fixture_execution".to_string(),
        records: eval_records.to_vec(),
    });
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
        dag_traces: &dag_traces,
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
        let artifact = describe_dag(&specs)
            .into_iter()
            .find(|spec| spec.name == "artifact_capture")
            .unwrap();

        assert!(artifact
            .dependencies
            .contains(&"benchmark_metrics".to_string()));
        assert!(artifact
            .dependencies
            .contains(&"reproduction_validation".to_string()));
    }

    #[test]
    fn build_stage_specs_keeps_reproduction_optional() {
        let specs = build_stage_specs(false);
        let descriptions = describe_dag(&specs);
        let reproduction = descriptions
            .iter()
            .find(|spec| spec.name == "reproduction_validation")
            .unwrap();
        let artifact = descriptions
            .iter()
            .find(|spec| spec.name == "artifact_capture")
            .unwrap();

        assert!(!reproduction.enabled);
        assert_eq!(artifact.dependencies, vec!["benchmark_metrics"]);
    }

    #[test]
    fn eval_fixture_graph_contract_exposes_reproduction_outputs() {
        let graph = describe_eval_fixture_graph(true);

        assert_eq!(graph.name, "eval_fixture_execution");
        assert_eq!(graph.entry_nodes, vec!["review"]);
        assert!(graph.nodes.iter().any(|node| {
            node.name == "review" && node.hints.subgraph.as_deref() == Some("review_pipeline")
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.name == "reproduction_validation"
                && node.outputs.contains(&"reproduction_summary".to_string())
        }));
    }
}
