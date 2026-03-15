#[path = "command/batch.rs"]
mod batch;
#[path = "command/fixtures.rs"]
mod fixtures;
#[path = "command/options.rs"]
mod options;
#[path = "command/report.rs"]
mod report;

use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::config;

use super::runner::prune_eval_artifacts;
use super::{EvalRunFilters, EvalRunMetadata, EvalRunOptions};
use batch::run_eval_batch;
use fixtures::run_eval_fixtures;
use options::{ensure_frontier_eval_models, prepare_eval_options};
use report::emit_eval_report;

pub async fn eval_command(
    mut config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
    mut options: EvalRunOptions,
) -> Result<()> {
    config.verification.fail_open = true;
    if options.trend_file.is_none() {
        options.trend_file = Some(config.eval_trend_path.clone());
    }
    if let Some(artifact_dir) = options.artifact_dir.as_deref() {
        let pruned =
            prune_eval_artifacts(artifact_dir, config.retention.eval_artifact_max_age_days).await?;
        if pruned > 0 {
            info!(
                artifact_dir = %artifact_dir.display(),
                pruned,
                "Pruned stale eval artifacts"
            );
        }
    }
    ensure_frontier_eval_models(&config, &options)?;
    if options.compare_agent_loop || options.repeat > 1 || !options.matrix_models.is_empty() {
        return run_eval_batch(config, &fixtures_dir, output_path.as_deref(), &options).await;
    }

    let execution = run_eval_fixtures(&config, &fixtures_dir, &options).await?;
    let prepared_options =
        prepare_eval_options(&options, config.retention.trend_history_max_entries)?;
    let report_output_path = output_path.clone().or_else(|| {
        options
            .artifact_dir
            .as_ref()
            .map(|dir| dir.join("report.json"))
    });
    let run_metadata = build_eval_run_metadata(
        &config,
        &fixtures_dir,
        &options,
        &execution,
        None,
        None,
        options.artifact_dir.as_deref(),
    );
    emit_eval_report(
        &config,
        execution.results,
        report_output_path.as_deref(),
        prepared_options,
        run_metadata,
    )
    .await
}

fn build_eval_run_metadata(
    config: &config::Config,
    fixtures_dir: &Path,
    options: &EvalRunOptions,
    execution: &fixtures::EvalFixtureExecution,
    repeat_index: Option<usize>,
    repeat_total: Option<usize>,
    artifact_dir: Option<&Path>,
) -> EvalRunMetadata {
    let (_, resolved_base_url, resolved_adapter) = config.resolve_provider();
    let provider = config.inferred_provider_label_for_role(config.generation_model_role);
    let generation_model = config.generation_model_name().to_string();
    let cost_breakdowns = crate::server::cost::aggregate_cost_breakdowns(
        execution
            .results
            .iter()
            .flat_map(|result| result.cost_breakdowns.clone()),
    );
    let mut verification_judges = Vec::new();
    let mut seen_verification_judges = HashSet::new();
    for role in std::iter::once(config.verification.model_role)
        .chain(config.verification.additional_model_roles.iter().copied())
    {
        let model = config.model_for_role(role).to_string();
        if seen_verification_judges.insert(model.clone()) {
            verification_judges.push(model);
        }
    }

    EvalRunMetadata {
        started_at: chrono::Utc::now().to_rfc3339(),
        fixtures_root: fixtures_dir.display().to_string(),
        fixtures_discovered: execution.discovered_count,
        fixtures_selected: execution.selected_count,
        label: options.label.clone(),
        comparison_group: options.comparison_group.clone(),
        model: generation_model,
        generation_model_role: Some(config.generation_model_role.as_str().to_string()),
        review_mode: review_mode_label(config.agent.enabled).to_string(),
        adapter: resolved_adapter.or_else(|| config.adapter.clone()),
        provider,
        base_url: resolved_base_url.or_else(|| config.base_url.clone()),
        filters: EvalRunFilters {
            suite_filters: options.suite_filters.clone(),
            category_filters: options.category_filters.clone(),
            language_filters: options.language_filters.clone(),
            fixture_name_filters: options.fixture_name_filters.clone(),
            max_fixtures: options.max_fixtures,
        },
        verification_fail_open: config.verification.fail_open,
        verification_judges,
        verification_consensus_mode: config
            .verification
            .enabled
            .then(|| config.verification.consensus_mode.as_str().to_string()),
        auditing_model: options
            .repro_validate
            .then(|| config.auditing_model_name().to_string()),
        auditing_model_role: options
            .repro_validate
            .then(|| config.auditing_model_role.as_str().to_string()),
        trend_file: options
            .trend_file
            .as_ref()
            .map(|path| path.display().to_string()),
        artifact_dir: artifact_dir.map(|path| path.display().to_string()),
        repeat_index,
        repeat_total,
        reproduction_validation: options.repro_validate,
        cost_breakdowns,
    }
}

fn review_mode_label(agent_enabled: bool) -> &'static str {
    if agent_enabled {
        "agent-loop"
    } else {
        "single-pass"
    }
}
