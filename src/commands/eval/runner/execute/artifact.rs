use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

use crate::core;
use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;

use super::super::super::{EvalFixtureMetadata, EvalRuleMetrics, EvalRuleScoreSummary};
use super::loading::PreparedFixtureExecution;

#[derive(Debug, Clone)]
pub(crate) struct EvalFixtureArtifactContext {
    pub(crate) artifact_dir: PathBuf,
    pub(crate) run_label: Option<String>,
    pub(crate) model: String,
}

pub(super) struct EvalFixtureArtifactInput<'a> {
    pub(super) context: Option<&'a EvalFixtureArtifactContext>,
    pub(super) prepared: &'a PreparedFixtureExecution,
    pub(super) total_comments: usize,
    pub(super) comments: &'a [core::Comment],
    pub(super) warnings: &'a [String],
    pub(super) failures: &'a [String],
    pub(super) benchmark_metrics: Option<&'a BenchmarkFixtureResult>,
    pub(super) rule_metrics: &'a [EvalRuleMetrics],
    pub(super) rule_summary: Option<EvalRuleScoreSummary>,
}

#[derive(Debug, Serialize)]
struct EvalFixtureArtifact {
    fixture: String,
    suite: Option<String>,
    fixture_path: String,
    repo_path: String,
    run_label: Option<String>,
    model: String,
    metadata: Option<EvalFixtureMetadata>,
    diff_content: String,
    total_comments: usize,
    comments: Vec<core::Comment>,
    warnings: Vec<String>,
    failures: Vec<String>,
    benchmark_metrics: Option<BenchmarkFixtureResult>,
    rule_metrics: Vec<EvalRuleMetrics>,
    rule_summary: Option<EvalRuleScoreSummary>,
}

pub(super) async fn maybe_write_fixture_artifact(
    input: EvalFixtureArtifactInput<'_>,
) -> Result<Option<String>> {
    let Some(context) = input.context else {
        return Ok(None);
    };
    if input.warnings.is_empty() && input.failures.is_empty() {
        return Ok(None);
    }

    let artifact_dir = context.artifact_dir.join("fixtures");
    tokio::fs::create_dir_all(&artifact_dir).await?;
    let artifact_path = artifact_dir.join(format!(
        "{}.json",
        sanitize_path_segment(&input.prepared.fixture_name)
    ));
    let artifact = EvalFixtureArtifact {
        fixture: input.prepared.fixture_name.clone(),
        suite: input.prepared.suite_name.clone(),
        fixture_path: input.prepared.fixture_path.display().to_string(),
        repo_path: input.prepared.repo_path.display().to_string(),
        run_label: context.run_label.clone(),
        model: context.model.clone(),
        metadata: input.prepared.metadata.clone(),
        diff_content: input.prepared.diff_content.clone(),
        total_comments: input.total_comments,
        comments: input.comments.to_vec(),
        warnings: input.warnings.to_vec(),
        failures: input.failures.to_vec(),
        benchmark_metrics: input.benchmark_metrics.cloned(),
        rule_metrics: input.rule_metrics.to_vec(),
        rule_summary: input.rule_summary,
    };
    tokio::fs::write(&artifact_path, serde_json::to_string_pretty(&artifact)?).await?;

    Ok(Some(artifact_path.display().to_string()))
}

fn sanitize_path_segment(value: &str) -> String {
    let mut sanitized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    let sanitized = sanitized
        .trim_matches('-')
        .chars()
        .take(120)
        .collect::<String>();
    if sanitized.is_empty() {
        "fixture".to_string()
    } else {
        sanitized
    }
}
