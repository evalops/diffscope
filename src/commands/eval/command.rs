#[path = "command/fixtures.rs"]
mod fixtures;
#[path = "command/options.rs"]
mod options;
#[path = "command/report.rs"]
mod report;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config;

use super::{EvalRunFilters, EvalRunMetadata, EvalRunOptions};
use fixtures::run_eval_fixtures;
use options::prepare_eval_options;
use report::emit_eval_report;

pub async fn eval_command(
    mut config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
    options: EvalRunOptions,
) -> Result<()> {
    config.verification_fail_open = true;
    let execution = run_eval_fixtures(&config, &fixtures_dir, &options).await?;
    let prepared_options = prepare_eval_options(&options)?;
    let run_metadata = build_eval_run_metadata(&config, &fixtures_dir, &options, &execution);
    emit_eval_report(
        execution.results,
        output_path.as_deref(),
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
) -> EvalRunMetadata {
    let (_, resolved_base_url, resolved_adapter) = config.resolve_provider();
    let provider = inferred_provider(
        resolved_base_url.as_deref().or(config.base_url.as_deref()),
        resolved_adapter.as_deref().or(config.adapter.as_deref()),
    );

    EvalRunMetadata {
        started_at: chrono::Utc::now().to_rfc3339(),
        fixtures_root: fixtures_dir.display().to_string(),
        fixtures_discovered: execution.discovered_count,
        fixtures_selected: execution.selected_count,
        label: options.label.clone(),
        model: config.model.clone(),
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
        verification_fail_open: config.verification_fail_open,
        trend_file: options
            .trend_file
            .as_ref()
            .map(|path| path.display().to_string()),
    }
}

fn inferred_provider(base_url: Option<&str>, adapter: Option<&str>) -> Option<String> {
    if base_url.is_some_and(|value| value.contains("openrouter.ai")) {
        return Some("openrouter".to_string());
    }

    adapter.map(|value| value.to_string())
}
