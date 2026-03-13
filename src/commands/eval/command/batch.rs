use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config;

use super::super::report::evaluation_failure_message;
use super::super::{EvalReport, EvalRunOptions};
use super::build_eval_run_metadata;
use super::fixtures::run_eval_fixtures;
use super::options::prepare_eval_options;
use super::report::materialize_eval_report;

#[derive(Debug, Serialize)]
struct EvalBatchModelSummary {
    model: String,
    provider: Option<String>,
    runs: usize,
    passing_runs: usize,
    average_micro_f1: Option<f32>,
    average_weighted_score: Option<f32>,
}

#[derive(Debug, Serialize)]
struct EvalBatchReport {
    generated_at: String,
    base_label: Option<String>,
    repeat: usize,
    models: Vec<String>,
    by_model: Vec<EvalBatchModelSummary>,
    runs: Vec<EvalReport>,
}

pub(super) async fn run_eval_batch(
    mut config: config::Config,
    fixtures_dir: &Path,
    output_path: Option<&Path>,
    options: &EvalRunOptions,
) -> Result<()> {
    config.verification.fail_open = true;
    let prepared_options = prepare_eval_options(options)?;
    let models = matrix_models(&config, options);
    let repeat_total = options.repeat.max(1);
    let multi_model = models.len() > 1;
    let repeating = repeat_total > 1;

    let mut reports = Vec::new();
    for model in &models {
        for repeat_index in 1..=repeat_total {
            let mut run_config = config.clone();
            run_config.model = model.clone();

            let mut run_options = options.clone();
            run_options.matrix_models.clear();
            run_options.repeat = 1;
            run_options.label = Some(build_run_label(
                options.label.as_deref(),
                model,
                repeat_index,
                repeat_total,
                multi_model,
                repeating,
            ));
            run_options.artifact_dir =
                batch_run_artifact_dir(options.artifact_dir.as_deref(), model, repeat_index);

            let execution = run_eval_fixtures(&run_config, fixtures_dir, &run_options).await?;
            let run_metadata = build_eval_run_metadata(
                &run_config,
                fixtures_dir,
                &run_options,
                &execution,
                repeating.then_some(repeat_index),
                repeating.then_some(repeat_total),
                run_options.artifact_dir.as_deref(),
            );
            let report_output_path = run_options
                .artifact_dir
                .as_ref()
                .map(|dir| dir.join("report.json"));
            let report = materialize_eval_report(
                execution.results,
                report_output_path.as_deref(),
                prepared_options.clone(),
                run_metadata,
                true,
            )
            .await?;
            reports.push(report);
        }
    }

    let batch_report = build_batch_report(options, repeat_total, models, reports);
    print_eval_batch_report(&batch_report);

    if let Some(path) = output_path {
        write_eval_batch_report(&batch_report, path).await?;
    }

    let failures = batch_report
        .runs
        .iter()
        .filter_map(|report| {
            evaluation_failure_message(report).map(|message| {
                let label = report
                    .run
                    .label
                    .clone()
                    .unwrap_or_else(|| report.run.model.clone());
                format!("{}: {}", label, message)
            })
        })
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        anyhow::bail!(
            "Batch evaluation failed for {} run(s): {}",
            failures.len(),
            failures.join(" | ")
        );
    }

    Ok(())
}

fn matrix_models(config: &config::Config, options: &EvalRunOptions) -> Vec<String> {
    let mut models = Vec::new();
    push_unique_model(&mut models, &config.model);
    for model in &options.matrix_models {
        let normalized = model.trim();
        if !normalized.is_empty() {
            push_unique_model(&mut models, normalized);
        }
    }
    models
}

fn push_unique_model(models: &mut Vec<String>, candidate: &str) {
    if !models.iter().any(|model| model == candidate) {
        models.push(candidate.to_string());
    }
}

fn build_run_label(
    base_label: Option<&str>,
    model: &str,
    repeat_index: usize,
    repeat_total: usize,
    multi_model: bool,
    repeating: bool,
) -> String {
    let prefix = base_label.unwrap_or("eval");
    match (multi_model, repeating) {
        (true, true) => format!(
            "{} [{} repeat {}/{}]",
            prefix, model, repeat_index, repeat_total
        ),
        (true, false) => format!("{} [{}]", prefix, model),
        (false, true) => format!("{} [repeat {}/{}]", prefix, repeat_index, repeat_total),
        (false, false) => prefix.to_string(),
    }
}

fn batch_run_artifact_dir(
    base_dir: Option<&Path>,
    model: &str,
    repeat_index: usize,
) -> Option<PathBuf> {
    let base_dir = base_dir?;
    Some(
        base_dir
            .join(sanitize_path_segment(model))
            .join(format!("repeat-{repeat_index:02}")),
    )
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
    sanitized
        .trim_matches('-')
        .to_string()
        .chars()
        .take(80)
        .collect::<String>()
        .if_empty_then("run")
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

fn build_batch_report(
    options: &EvalRunOptions,
    repeat_total: usize,
    models: Vec<String>,
    runs: Vec<EvalReport>,
) -> EvalBatchReport {
    let mut grouped = BTreeMap::<String, Vec<&EvalReport>>::new();
    for report in &runs {
        grouped
            .entry(report.run.model.clone())
            .or_default()
            .push(report);
    }

    let by_model = grouped
        .into_iter()
        .map(|(model, reports)| {
            let micro_f1_values = reports
                .iter()
                .filter_map(|report| {
                    report
                        .benchmark_summary
                        .as_ref()
                        .map(|metrics| metrics.micro_f1)
                })
                .collect::<Vec<_>>();
            let weighted_values = reports
                .iter()
                .filter_map(|report| {
                    report
                        .benchmark_summary
                        .as_ref()
                        .map(|metrics| metrics.weighted_score)
                })
                .collect::<Vec<_>>();
            EvalBatchModelSummary {
                model,
                provider: reports
                    .first()
                    .and_then(|report| report.run.provider.clone()),
                runs: reports.len(),
                passing_runs: reports
                    .iter()
                    .filter(|report| evaluation_failure_message(report).is_none())
                    .count(),
                average_micro_f1: average(&micro_f1_values),
                average_weighted_score: average(&weighted_values),
            }
        })
        .collect::<Vec<_>>();

    EvalBatchReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        base_label: options.label.clone(),
        repeat: repeat_total,
        models,
        by_model,
        runs,
    }
}

fn average(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

fn print_eval_batch_report(report: &EvalBatchReport) {
    println!(
        "Eval batch summary: {} run(s) across {} model(s)",
        report.runs.len(),
        report.models.len()
    );
    for summary in &report.by_model {
        println!(
            "  - {}: {}/{} passing | avg micro F1={} avg weighted={}",
            summary.model,
            summary.passing_runs,
            summary.runs,
            percentage_or_na(summary.average_micro_f1),
            percentage_or_na(summary.average_weighted_score)
        );
    }
}

fn percentage_or_na(value: Option<f32>) -> String {
    value
        .map(|value| format!("{:.0}%", value * 100.0))
        .unwrap_or_else(|| "n/a".to_string())
}

async fn write_eval_batch_report(report: &EvalBatchReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serde_json::to_string_pretty(report)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_options() -> EvalRunOptions {
        EvalRunOptions {
            baseline_report: None,
            max_micro_f1_drop: None,
            max_suite_f1_drop: None,
            max_category_f1_drop: None,
            max_language_f1_drop: None,
            min_micro_f1: None,
            min_macro_f1: None,
            min_rule_f1: vec![],
            max_rule_f1_drop: vec![],
            matrix_models: vec![],
            repeat: 1,
            suite_filters: vec![],
            category_filters: vec![],
            language_filters: vec![],
            fixture_name_filters: vec![],
            max_fixtures: None,
            label: Some("smoke".to_string()),
            trend_file: None,
            artifact_dir: None,
        }
    }

    #[test]
    fn matrix_models_includes_primary_model_once() {
        let config = config::Config {
            model: "anthropic/claude-opus-4.1".to_string(),
            ..Default::default()
        };
        let mut options = sample_options();
        options.matrix_models = vec![
            "anthropic/claude-opus-4.1".to_string(),
            "openai/o3".to_string(),
        ];

        let models = matrix_models(&config, &options);

        assert_eq!(
            models,
            vec![
                "anthropic/claude-opus-4.1".to_string(),
                "openai/o3".to_string()
            ]
        );
    }

    #[test]
    fn build_run_label_adds_matrix_and_repeat_context() {
        let label = build_run_label(Some("depth"), "anthropic/claude-opus-4.1", 2, 3, true, true);

        assert_eq!(label, "depth [anthropic/claude-opus-4.1 repeat 2/3]");
    }

    #[test]
    fn batch_run_artifact_dir_sanitizes_model_name() {
        let artifact_dir = batch_run_artifact_dir(
            Some(Path::new("/tmp/eval-artifacts")),
            "anthropic/claude-opus-4.1",
            2,
        )
        .unwrap();

        assert_eq!(
            artifact_dir,
            PathBuf::from("/tmp/eval-artifacts/anthropic-claude-opus-4-1/repeat-02")
        );
    }
}
