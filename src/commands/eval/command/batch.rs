use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config;

use super::super::metrics::{
    build_report_usefulness_signals, compute_usefulness_score, EvalUsefulnessSignals,
};
use super::super::report::evaluation_failure_message;
use super::super::{EvalReport, EvalRunOptions};
use super::build_eval_run_metadata;
use super::fixtures::run_eval_fixtures;
use super::options::prepare_eval_options;
use super::report::materialize_eval_report;
use super::review_mode_label;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalBatchReviewMode {
    SinglePass,
    AgentLoop,
}

impl EvalBatchReviewMode {
    fn from_agent_enabled(agent_enabled: bool) -> Self {
        if agent_enabled {
            Self::AgentLoop
        } else {
            Self::SinglePass
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SinglePass => "single-pass",
            Self::AgentLoop => "agent-loop",
        }
    }

    fn agent_enabled(self) -> bool {
        matches!(self, Self::AgentLoop)
    }
}

#[derive(Debug, Serialize)]
struct EvalBatchReviewModeSummary {
    review_mode: String,
    runs: usize,
    passing_runs: usize,
    average_micro_f1: Option<f32>,
    average_weighted_score: Option<f32>,
    average_agent_iterations: Option<f32>,
    average_tool_calls: Option<f32>,
    runs_with_agent_activity: usize,
}

#[derive(Debug, Serialize)]
struct EvalBatchReviewModeComparison {
    baseline_review_mode: String,
    compare_review_mode: String,
    baseline_micro_f1: Option<f32>,
    current_micro_f1: Option<f32>,
    micro_f1_delta: Option<f32>,
    baseline_weighted_score: Option<f32>,
    current_weighted_score: Option<f32>,
    weighted_score_delta: Option<f32>,
    baseline_pass_rate: f32,
    current_pass_rate: f32,
    pass_rate_delta: f32,
}

#[derive(Debug, Serialize)]
struct EvalReviewerLeaderboardEntry {
    rank: usize,
    reviewer: String,
    model: String,
    provider: Option<String>,
    review_mode: String,
    runs: usize,
    passing_runs: usize,
    pass_rate: f32,
    average_micro_f1: Option<f32>,
    average_weighted_score: Option<f32>,
    average_verification_health: Option<f32>,
    average_lifecycle_accuracy: Option<f32>,
    usefulness_score: f32,
    provisional: bool,
}

#[derive(Debug, Serialize)]
struct EvalIndependentAuditorStory {
    benchmark_label: String,
    winning_reviewer: String,
    winning_model: String,
    winning_provider: Option<String>,
    winning_review_mode: String,
    winning_usefulness_score: f32,
    winning_weighted_score: Option<f32>,
    winning_micro_f1: Option<f32>,
    winning_pass_rate: f32,
    winning_verification_health: Option<f32>,
    winning_lifecycle_accuracy: Option<f32>,
    winning_provisional: bool,
    review_mode_comparison: Option<EvalIndependentAuditorStoryComparison>,
}

#[derive(Debug, Serialize)]
struct EvalIndependentAuditorStoryComparison {
    baseline_review_mode: String,
    compare_review_mode: String,
    micro_f1_delta: Option<f32>,
    weighted_score_delta: Option<f32>,
    pass_rate_delta: f32,
    usefulness_score_delta: f32,
}

#[derive(Debug, Serialize)]
struct EvalBatchModelSummary {
    model: String,
    provider: Option<String>,
    runs: usize,
    passing_runs: usize,
    average_micro_f1: Option<f32>,
    average_weighted_score: Option<f32>,
    by_review_mode: Vec<EvalBatchReviewModeSummary>,
    review_mode_comparisons: Vec<EvalBatchReviewModeComparison>,
}

#[derive(Debug, Serialize)]
struct EvalBatchReport {
    generated_at: String,
    base_label: Option<String>,
    repeat: usize,
    models: Vec<String>,
    review_modes: Vec<String>,
    leaderboard: Vec<EvalReviewerLeaderboardEntry>,
    independent_auditor_story: Option<EvalIndependentAuditorStory>,
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
    let prepared_options =
        prepare_eval_options(options, config.retention.trend_history_max_entries)?;
    let models = matrix_models(&config, options);
    let review_modes = batch_review_modes(&config, options);
    let repeat_total = options.repeat.max(1);
    let multi_model = models.len() > 1;
    let multi_review_mode = review_modes.len() > 1;
    let repeating = repeat_total > 1;

    let mut reports = Vec::new();
    for model in &models {
        for review_mode in &review_modes {
            for repeat_index in 1..=repeat_total {
                let mut run_config = config.clone();
                run_config.set_model_for_role(run_config.generation_model_role, model.clone());
                run_config.agent.enabled = review_mode.agent_enabled();

                let mut run_options = options.clone();
                run_options.compare_agent_loop = false;
                run_options.matrix_models.clear();
                run_options.repeat = 1;
                run_options.comparison_group =
                    Some(options.label.clone().unwrap_or_else(|| "eval".to_string()));
                run_options.label = Some(build_run_label(
                    options.label.as_deref(),
                    model,
                    *review_mode,
                    repeating.then_some((repeat_index, repeat_total)),
                    multi_model,
                    multi_review_mode,
                ));
                run_options.artifact_dir = batch_run_artifact_dir(
                    options.artifact_dir.as_deref(),
                    model,
                    *review_mode,
                    repeat_index,
                    multi_review_mode,
                );

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
    }

    let batch_report = build_batch_report(
        options,
        repeat_total,
        models,
        review_modes
            .iter()
            .map(|mode| mode.as_str().to_string())
            .collect(),
        reports,
    );
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
                format!("{label}: {message}")
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

fn batch_review_modes(
    config: &config::Config,
    options: &EvalRunOptions,
) -> Vec<EvalBatchReviewMode> {
    if options.compare_agent_loop {
        vec![
            EvalBatchReviewMode::SinglePass,
            EvalBatchReviewMode::AgentLoop,
        ]
    } else {
        vec![EvalBatchReviewMode::from_agent_enabled(
            config.agent.enabled,
        )]
    }
}

fn matrix_models(config: &config::Config, options: &EvalRunOptions) -> Vec<String> {
    let mut models = Vec::new();
    push_unique_model(&mut models, config.generation_model_name());
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
    review_mode: EvalBatchReviewMode,
    repeat: Option<(usize, usize)>,
    multi_model: bool,
    multi_review_mode: bool,
) -> String {
    let prefix = base_label.unwrap_or("eval");
    let mut qualifiers = Vec::new();
    if multi_review_mode || review_mode == EvalBatchReviewMode::AgentLoop {
        qualifiers.push(review_mode.as_str().to_string());
    }
    if multi_model {
        qualifiers.push(model.to_string());
    }
    if let Some((repeat_index, repeat_total)) = repeat {
        qualifiers.push(format!("repeat {repeat_index}/{repeat_total}"));
    }

    if qualifiers.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} [{}]", qualifiers.join(" "))
    }
}

fn batch_run_artifact_dir(
    base_dir: Option<&Path>,
    model: &str,
    review_mode: EvalBatchReviewMode,
    repeat_index: usize,
    multi_review_mode: bool,
) -> Option<PathBuf> {
    let base_dir = base_dir?;
    let mut path = base_dir.join(sanitize_path_segment(model));
    if multi_review_mode || review_mode == EvalBatchReviewMode::AgentLoop {
        path = path.join(review_mode.as_str());
    }
    Some(path.join(format!("repeat-{repeat_index:02}")))
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
    review_modes: Vec<String>,
    runs: Vec<EvalReport>,
) -> EvalBatchReport {
    let leaderboard = build_reviewer_leaderboard(&runs);
    let by_model = models
        .iter()
        .map(|model| {
            let reports = runs
                .iter()
                .filter(|report| report.run.model == *model)
                .collect::<Vec<_>>();
            let by_review_mode = review_modes
                .iter()
                .filter_map(|review_mode| {
                    let mode_reports = reports
                        .iter()
                        .copied()
                        .filter(|report| report_review_mode(report) == review_mode)
                        .collect::<Vec<_>>();
                    (!mode_reports.is_empty())
                        .then(|| build_review_mode_summary(review_mode, &mode_reports))
                })
                .collect::<Vec<_>>();
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
                model: model.clone(),
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
                review_mode_comparisons: build_review_mode_comparisons(&by_review_mode),
                by_review_mode,
            }
        })
        .collect::<Vec<_>>();

    EvalBatchReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        base_label: options.label.clone(),
        repeat: repeat_total,
        models,
        review_modes,
        independent_auditor_story: build_independent_auditor_story(options, &leaderboard),
        leaderboard,
        by_model,
        runs,
    }
}

fn build_reviewer_leaderboard(runs: &[EvalReport]) -> Vec<EvalReviewerLeaderboardEntry> {
    let mut grouped = BTreeMap::<(String, Option<String>, String), Vec<&EvalReport>>::new();
    for report in runs {
        grouped
            .entry((
                report.run.model.clone(),
                report.run.provider.clone(),
                report_review_mode(report).to_string(),
            ))
            .or_default()
            .push(report);
    }

    let mut leaderboard = grouped
        .into_iter()
        .map(|((model, provider, review_mode), reports)| {
            let average_signals = average_usefulness_signals(&reports);
            let passing_runs = reports
                .iter()
                .filter(|report| evaluation_failure_message(report).is_none())
                .count();
            let pass_rate = pass_rate(passing_runs, reports.len());

            EvalReviewerLeaderboardEntry {
                rank: 0,
                reviewer: reviewer_identity(&model, provider.as_deref(), &review_mode),
                model,
                provider,
                review_mode,
                runs: reports.len(),
                passing_runs,
                pass_rate,
                average_micro_f1: average_signals.micro_f1,
                average_weighted_score: average_signals.weighted_score,
                average_verification_health: average_signals.verification_health,
                average_lifecycle_accuracy: average_signals.lifecycle_accuracy,
                usefulness_score: compute_usefulness_score(EvalUsefulnessSignals {
                    pass_rate,
                    ..average_signals
                }),
                provisional: reports.len() < 2,
            }
        })
        .collect::<Vec<_>>();

    leaderboard.sort_by(|left, right| {
        right
            .usefulness_score
            .total_cmp(&left.usefulness_score)
            .then_with(|| {
                right
                    .average_weighted_score
                    .unwrap_or_default()
                    .total_cmp(&left.average_weighted_score.unwrap_or_default())
            })
            .then_with(|| right.pass_rate.total_cmp(&left.pass_rate))
            .then_with(|| left.reviewer.cmp(&right.reviewer))
    });

    for (index, entry) in leaderboard.iter_mut().enumerate() {
        entry.rank = index + 1;
    }

    leaderboard
}

fn reviewer_identity(model: &str, provider: Option<&str>, review_mode: &str) -> String {
    match provider {
        Some(provider) => format!("{model} via {provider} [{review_mode}]"),
        None => format!("{model} [{review_mode}]"),
    }
}

fn build_independent_auditor_story(
    options: &EvalRunOptions,
    leaderboard: &[EvalReviewerLeaderboardEntry],
) -> Option<EvalIndependentAuditorStory> {
    let winner = leaderboard.first()?;
    let single_pass = leaderboard.iter().find(|entry| {
        entry.model == winner.model
            && entry.provider == winner.provider
            && entry.review_mode == review_mode_label(false)
    });
    let agent_loop = leaderboard.iter().find(|entry| {
        entry.model == winner.model
            && entry.provider == winner.provider
            && entry.review_mode == review_mode_label(true)
    });

    Some(EvalIndependentAuditorStory {
        benchmark_label: options.label.clone().unwrap_or_else(|| "eval".to_string()),
        winning_reviewer: winner.reviewer.clone(),
        winning_model: winner.model.clone(),
        winning_provider: winner.provider.clone(),
        winning_review_mode: winner.review_mode.clone(),
        winning_usefulness_score: winner.usefulness_score,
        winning_weighted_score: winner.average_weighted_score,
        winning_micro_f1: winner.average_micro_f1,
        winning_pass_rate: winner.pass_rate,
        winning_verification_health: winner.average_verification_health,
        winning_lifecycle_accuracy: winner.average_lifecycle_accuracy,
        winning_provisional: winner.provisional,
        review_mode_comparison: single_pass
            .zip(agent_loop)
            .map(
                |(single_pass, agent_loop)| EvalIndependentAuditorStoryComparison {
                    baseline_review_mode: single_pass.review_mode.clone(),
                    compare_review_mode: agent_loop.review_mode.clone(),
                    micro_f1_delta: delta(
                        agent_loop.average_micro_f1,
                        single_pass.average_micro_f1,
                    ),
                    weighted_score_delta: delta(
                        agent_loop.average_weighted_score,
                        single_pass.average_weighted_score,
                    ),
                    pass_rate_delta: agent_loop.pass_rate - single_pass.pass_rate,
                    usefulness_score_delta: agent_loop.usefulness_score
                        - single_pass.usefulness_score,
                },
            ),
    })
}

fn average_usefulness_signals(reports: &[&EvalReport]) -> EvalUsefulnessSignals {
    let mut micro_f1 = Vec::new();
    let mut weighted_score = Vec::new();
    let mut verification_health = Vec::new();
    let mut lifecycle_accuracy = Vec::new();

    for report in reports {
        let signals = build_report_usefulness_signals(report);
        if let Some(value) = signals.micro_f1 {
            micro_f1.push(value);
        }
        if let Some(value) = signals.weighted_score {
            weighted_score.push(value);
        }
        if let Some(value) = signals.verification_health {
            verification_health.push(value);
        }
        if let Some(value) = signals.lifecycle_accuracy {
            lifecycle_accuracy.push(value);
        }
    }

    EvalUsefulnessSignals {
        micro_f1: average(&micro_f1),
        weighted_score: average(&weighted_score),
        verification_health: average(&verification_health),
        lifecycle_accuracy: average(&lifecycle_accuracy),
        ..Default::default()
    }
}

fn build_review_mode_summary(
    review_mode: &str,
    reports: &[&EvalReport],
) -> EvalBatchReviewModeSummary {
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
    let agent_activity = reports
        .iter()
        .filter_map(|report| total_agent_activity(report))
        .collect::<Vec<_>>();
    let agent_iterations = agent_activity
        .iter()
        .map(|(iterations, _)| *iterations as f32)
        .collect::<Vec<_>>();
    let tool_calls = agent_activity
        .iter()
        .map(|(_, tool_calls)| *tool_calls as f32)
        .collect::<Vec<_>>();

    EvalBatchReviewModeSummary {
        review_mode: review_mode.to_string(),
        runs: reports.len(),
        passing_runs: reports
            .iter()
            .filter(|report| evaluation_failure_message(report).is_none())
            .count(),
        average_micro_f1: average(&micro_f1_values),
        average_weighted_score: average(&weighted_values),
        average_agent_iterations: average(&agent_iterations),
        average_tool_calls: average(&tool_calls),
        runs_with_agent_activity: agent_activity.len(),
    }
}

fn build_review_mode_comparisons(
    by_review_mode: &[EvalBatchReviewModeSummary],
) -> Vec<EvalBatchReviewModeComparison> {
    let Some(single_pass) = by_review_mode
        .iter()
        .find(|summary| summary.review_mode == review_mode_label(false))
    else {
        return Vec::new();
    };
    let Some(agent_loop) = by_review_mode
        .iter()
        .find(|summary| summary.review_mode == review_mode_label(true))
    else {
        return Vec::new();
    };

    let baseline_pass_rate = pass_rate(single_pass.passing_runs, single_pass.runs);
    let current_pass_rate = pass_rate(agent_loop.passing_runs, agent_loop.runs);
    vec![EvalBatchReviewModeComparison {
        baseline_review_mode: single_pass.review_mode.clone(),
        compare_review_mode: agent_loop.review_mode.clone(),
        baseline_micro_f1: single_pass.average_micro_f1,
        current_micro_f1: agent_loop.average_micro_f1,
        micro_f1_delta: delta(agent_loop.average_micro_f1, single_pass.average_micro_f1),
        baseline_weighted_score: single_pass.average_weighted_score,
        current_weighted_score: agent_loop.average_weighted_score,
        weighted_score_delta: delta(
            agent_loop.average_weighted_score,
            single_pass.average_weighted_score,
        ),
        baseline_pass_rate,
        current_pass_rate,
        pass_rate_delta: current_pass_rate - baseline_pass_rate,
    }]
}

fn report_review_mode(report: &EvalReport) -> &str {
    if report.run.review_mode.is_empty() {
        if total_agent_activity(report).is_some() {
            review_mode_label(true)
        } else {
            review_mode_label(false)
        }
    } else {
        &report.run.review_mode
    }
}

fn total_agent_activity(report: &EvalReport) -> Option<(usize, usize)> {
    let mut total_iterations = 0;
    let mut total_tool_calls = 0;
    let mut saw_activity = false;

    for result in &report.results {
        if let Some(activity) = result.agent_activity.as_ref() {
            saw_activity = true;
            total_iterations += activity.total_iterations;
            total_tool_calls += activity.tool_calls.len();
        }
    }

    saw_activity.then_some((total_iterations, total_tool_calls))
}

fn delta(current: Option<f32>, baseline: Option<f32>) -> Option<f32> {
    current
        .zip(baseline)
        .map(|(current, baseline)| current - baseline)
}

fn pass_rate(passing_runs: usize, runs: usize) -> f32 {
    if runs == 0 {
        0.0
    } else {
        passing_runs as f32 / runs as f32
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
        if report.review_modes.len() > 1
            || report
                .review_modes
                .first()
                .is_some_and(|mode| mode == review_mode_label(true))
        {
            for review_mode in &summary.by_review_mode {
                println!(
                    "      {}: {}/{} passing | avg micro F1={} avg weighted={} avg agent iters={} avg tool-calls={}",
                    review_mode.review_mode,
                    review_mode.passing_runs,
                    review_mode.runs,
                    percentage_or_na(review_mode.average_micro_f1),
                    percentage_or_na(review_mode.average_weighted_score),
                    decimal_or_na(review_mode.average_agent_iterations),
                    decimal_or_na(review_mode.average_tool_calls)
                );
                if review_mode.review_mode == review_mode_label(true)
                    && review_mode.runs_with_agent_activity == 0
                {
                    println!("        note: requested agent-loop runs showed no tool activity");
                }
            }
        }
        for comparison in &summary.review_mode_comparisons {
            println!(
                "      compare {} vs {}: micro F1 {} weighted {} pass rate {:+.0}%",
                comparison.compare_review_mode,
                comparison.baseline_review_mode,
                signed_percentage_or_na(comparison.micro_f1_delta),
                signed_percentage_or_na(comparison.weighted_score_delta),
                comparison.pass_rate_delta * 100.0
            );
        }
    }

    if !report.leaderboard.is_empty() {
        println!("Reviewer usefulness leaderboard:");
        for entry in &report.leaderboard {
            println!(
                "  {}. {} | usefulness={} weighted={} micro F1={} pass={} verification={} lifecycle={} runs={}{}",
                entry.rank,
                entry.reviewer,
                percentage(entry.usefulness_score),
                percentage_or_na(entry.average_weighted_score),
                percentage_or_na(entry.average_micro_f1),
                percentage(entry.pass_rate),
                percentage_or_na(entry.average_verification_health),
                percentage_or_na(entry.average_lifecycle_accuracy),
                entry.runs,
                if entry.provisional { " provisional" } else { "" }
            );
        }
    }

    if let Some(story) = report.independent_auditor_story.as_ref() {
        println!("Independent auditor benchmark ({}):", story.benchmark_label);
        println!(
            "  winner: {} | usefulness={} weighted={} micro F1={} pass={} verification={} lifecycle={}{}",
            story.winning_reviewer,
            percentage(story.winning_usefulness_score),
            percentage_or_na(story.winning_weighted_score),
            percentage_or_na(story.winning_micro_f1),
            percentage(story.winning_pass_rate),
            percentage_or_na(story.winning_verification_health),
            percentage_or_na(story.winning_lifecycle_accuracy),
            if story.winning_provisional {
                " provisional"
            } else {
                ""
            }
        );
        if let Some(comparison) = story.review_mode_comparison.as_ref() {
            println!(
                "  {} vs {}: usefulness {} weighted {} micro F1 {} pass rate {:+.0}%",
                comparison.compare_review_mode,
                comparison.baseline_review_mode,
                format_args!("{:+.0}%", comparison.usefulness_score_delta * 100.0),
                signed_percentage_or_na(comparison.weighted_score_delta),
                signed_percentage_or_na(comparison.micro_f1_delta),
                comparison.pass_rate_delta * 100.0
            );
        }
    }
}

fn percentage_or_na(value: Option<f32>) -> String {
    value.map(percentage).unwrap_or_else(|| "n/a".to_string())
}

fn percentage(value: f32) -> String {
    format!("{:.0}%", value * 100.0)
}

fn signed_percentage_or_na(value: Option<f32>) -> String {
    value
        .map(|value| format!("{:+.0}%", value * 100.0))
        .unwrap_or_else(|| "n/a".to_string())
}

fn decimal_or_na(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.1}"))
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
    use crate::commands::eval::{
        EvalAgentActivity, EvalAgentToolCall, EvalFixtureResult, EvalRuleMetrics, EvalRunMetadata,
        EvalVerificationHealth,
    };
    use crate::core::eval_benchmarks::AggregateMetrics;

    use super::*;

    fn sample_options() -> EvalRunOptions {
        EvalRunOptions {
            baseline_report: None,
            compare_agent_loop: false,
            max_micro_f1_drop: None,
            max_suite_f1_drop: None,
            max_category_f1_drop: None,
            max_language_f1_drop: None,
            min_micro_f1: None,
            min_macro_f1: None,
            min_verification_health: None,
            min_lifecycle_accuracy: None,
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
            comparison_group: None,
            trend_file: None,
            artifact_dir: None,
            allow_subfrontier_models: false,
            repro_validate: false,
            repro_max_comments: 3,
        }
    }

    struct SampleReportInput<'a> {
        model: &'a str,
        review_mode: &'a str,
        micro_f1: f32,
        weighted_score: f32,
        passing: bool,
        agent_iterations: Option<usize>,
        tool_calls: usize,
        verification_health: Option<f32>,
        lifecycle_passed: Option<bool>,
    }

    fn sample_report(input: SampleReportInput<'_>) -> EvalReport {
        EvalReport {
            run: EvalRunMetadata {
                model: input.model.to_string(),
                provider: Some("openrouter".to_string()),
                review_mode: input.review_mode.to_string(),
                ..Default::default()
            },
            fixtures_total: 1,
            fixtures_passed: usize::from(input.passing),
            fixtures_failed: usize::from(!input.passing),
            rule_metrics: vec![],
            rule_summary: None,
            benchmark_summary: Some(AggregateMetrics {
                fixture_count: 1,
                micro_f1: input.micro_f1,
                weighted_score: input.weighted_score,
                ..Default::default()
            }),
            suite_results: vec![],
            benchmark_by_category: Default::default(),
            benchmark_by_language: Default::default(),
            benchmark_by_difficulty: Default::default(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            verification_health: input.verification_health.map(|verified_pct| {
                EvalVerificationHealth {
                    verified_checks: 1,
                    total_checks: 1,
                    verified_pct,
                    warnings_total: 0,
                    fixtures_with_warnings: 0,
                    fail_open_warning_count: 0,
                    parse_failure_count: 0,
                    request_failure_count: 0,
                }
            }),
            warnings: vec![],
            threshold_failures: if input.passing {
                vec![]
            } else {
                vec!["threshold failed".to_string()]
            },
            results: vec![EvalFixtureResult {
                passed: input.passing,
                agent_activity: input
                    .agent_iterations
                    .map(|total_iterations| EvalAgentActivity {
                        total_iterations,
                        tool_calls: (0..input.tool_calls)
                            .map(|index| EvalAgentToolCall {
                                iteration: 1,
                                tool_name: format!("tool-{index}"),
                                duration_ms: 1,
                            })
                            .collect(),
                    }),
                rule_metrics: input
                    .lifecycle_passed
                    .map(|passed| EvalRuleMetrics {
                        rule_id: "bug.lifecycle.context-only-addressed".to_string(),
                        expected: 1,
                        predicted: usize::from(passed),
                        true_positives: usize::from(passed),
                        false_positives: 0,
                        false_negatives: usize::from(!passed),
                        precision: if passed { 1.0 } else { 0.0 },
                        recall: if passed { 1.0 } else { 0.0 },
                        f1: if passed { 1.0 } else { 0.0 },
                    })
                    .into_iter()
                    .collect(),
                ..Default::default()
            }],
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
        let label = build_run_label(
            Some("depth"),
            "anthropic/claude-opus-4.1",
            EvalBatchReviewMode::SinglePass,
            Some((2, 3)),
            true,
            false,
        );

        assert_eq!(label, "depth [anthropic/claude-opus-4.1 repeat 2/3]");
    }

    #[test]
    fn batch_run_artifact_dir_sanitizes_model_name() {
        let artifact_dir = batch_run_artifact_dir(
            Some(Path::new("/tmp/eval-artifacts")),
            "anthropic/claude-opus-4.1",
            EvalBatchReviewMode::SinglePass,
            2,
            false,
        )
        .unwrap();

        assert_eq!(
            artifact_dir,
            PathBuf::from("/tmp/eval-artifacts/anthropic-claude-opus-4-1/repeat-02")
        );
    }

    #[test]
    fn build_run_label_includes_agent_mode_when_enabled() {
        let label = build_run_label(
            Some("depth"),
            "anthropic/claude-opus-4.1",
            EvalBatchReviewMode::AgentLoop,
            Some((2, 3)),
            false,
            false,
        );

        assert_eq!(label, "depth [agent-loop repeat 2/3]");
    }

    #[test]
    fn batch_run_artifact_dir_includes_review_mode_when_comparing() {
        let artifact_dir = batch_run_artifact_dir(
            Some(Path::new("/tmp/eval-artifacts")),
            "anthropic/claude-opus-4.1",
            EvalBatchReviewMode::AgentLoop,
            1,
            true,
        )
        .unwrap();

        assert_eq!(
            artifact_dir,
            PathBuf::from("/tmp/eval-artifacts/anthropic-claude-opus-4-1/agent-loop/repeat-01")
        );
    }

    #[test]
    fn build_batch_report_adds_agent_loop_comparison() {
        let report = build_batch_report(
            &sample_options(),
            1,
            vec!["anthropic/claude-opus-4.5".to_string()],
            vec![
                review_mode_label(false).to_string(),
                review_mode_label(true).to_string(),
            ],
            vec![
                sample_report(SampleReportInput {
                    model: "anthropic/claude-opus-4.5",
                    review_mode: review_mode_label(false),
                    micro_f1: 0.6,
                    weighted_score: 0.5,
                    passing: false,
                    agent_iterations: None,
                    tool_calls: 0,
                    verification_health: Some(0.7),
                    lifecycle_passed: Some(false),
                }),
                sample_report(SampleReportInput {
                    model: "anthropic/claude-opus-4.5",
                    review_mode: review_mode_label(true),
                    micro_f1: 0.8,
                    weighted_score: 0.7,
                    passing: true,
                    agent_iterations: Some(4),
                    tool_calls: 3,
                    verification_health: Some(0.9),
                    lifecycle_passed: Some(true),
                }),
            ],
        );

        assert_eq!(report.by_model.len(), 1);
        assert_eq!(report.by_model[0].by_review_mode.len(), 2);
        assert_eq!(report.by_model[0].review_mode_comparisons.len(), 1);
        assert_eq!(report.leaderboard.len(), 2);
        assert_eq!(report.leaderboard[0].review_mode, review_mode_label(true));
        assert!(report.leaderboard[0].usefulness_score > report.leaderboard[1].usefulness_score);
        assert_eq!(report.leaderboard[0].average_verification_health, Some(0.9));
        assert_eq!(report.leaderboard[0].average_lifecycle_accuracy, Some(1.0));
        assert_eq!(
            report
                .independent_auditor_story
                .as_ref()
                .map(|story| story.winning_review_mode.as_str()),
            Some(review_mode_label(true))
        );
        assert_eq!(
            report
                .independent_auditor_story
                .as_ref()
                .and_then(|story| story.review_mode_comparison.as_ref())
                .map(|comparison| comparison.usefulness_score_delta > 0.0),
            Some(true)
        );
        assert!(
            (report.by_model[0].review_mode_comparisons[0]
                .micro_f1_delta
                .unwrap_or_default()
                - 0.2)
                .abs()
                < f32::EPSILON
        );
        assert!(
            (report.by_model[0].review_mode_comparisons[0]
                .weighted_score_delta
                .unwrap_or_default()
                - 0.2)
                .abs()
                < f32::EPSILON
        );
        assert_eq!(
            report.by_model[0].by_review_mode[1].average_agent_iterations,
            Some(4.0)
        );
        assert_eq!(
            report.by_model[0].by_review_mode[1].average_tool_calls,
            Some(3.0)
        );
    }

    #[test]
    fn reviewer_leaderboard_marks_single_run_entries_provisional() {
        let leaderboard = build_reviewer_leaderboard(&[sample_report(SampleReportInput {
            model: "openai/o3",
            review_mode: review_mode_label(false),
            micro_f1: 0.75,
            weighted_score: 0.7,
            passing: true,
            agent_iterations: None,
            tool_calls: 0,
            verification_health: None,
            lifecycle_passed: None,
        })]);

        assert_eq!(leaderboard.len(), 1);
        assert!(leaderboard[0].provisional);
        assert_eq!(leaderboard[0].rank, 1);
    }
}
