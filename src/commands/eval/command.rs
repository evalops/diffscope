use anyhow::Result;
use std::path::PathBuf;

use crate::config;

use super::fixtures::{collect_eval_fixtures, load_eval_report};
use super::report::{
    build_eval_report, evaluation_failure_message, print_eval_report, write_eval_report,
};
use super::runner::run_eval_fixture;
use super::thresholds::{parse_rule_threshold_args, EvalThresholdOptions};
use super::EvalRunOptions;

pub async fn eval_command(
    config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
    options: EvalRunOptions,
) -> Result<()> {
    let fixtures = collect_eval_fixtures(&fixtures_dir)?;
    if fixtures.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let mut results = Vec::new();
    for fixture in fixtures {
        results.push(run_eval_fixture(&config, fixture).await?);
    }

    let baseline = match options.baseline_report.as_deref() {
        Some(path) => Some(load_eval_report(path)?),
        None => None,
    };
    let min_rule_thresholds = parse_rule_threshold_args(&options.min_rule_f1, "min-rule-f1")?;
    let max_rule_drop_thresholds =
        parse_rule_threshold_args(&options.max_rule_f1_drop, "max-rule-f1-drop")?;
    let threshold_options = EvalThresholdOptions {
        max_micro_f1_drop: options.max_micro_f1_drop,
        min_micro_f1: options.min_micro_f1,
        min_macro_f1: options.min_macro_f1,
        min_rule_f1: min_rule_thresholds,
        max_rule_f1_drop: max_rule_drop_thresholds,
    };

    let report = build_eval_report(results, baseline.as_ref(), &threshold_options);
    print_eval_report(&report);

    if let Some(path) = output_path.as_deref() {
        write_eval_report(&report, path).await?;
    }

    if let Some(message) = evaluation_failure_message(&report) {
        anyhow::bail!("{}", message);
    }

    Ok(())
}
