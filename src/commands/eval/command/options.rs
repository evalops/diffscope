use anyhow::Result;

use super::super::fixtures::load_eval_report;
use super::super::thresholds::{parse_rule_threshold_args, EvalThresholdOptions};
use super::super::{EvalReport, EvalRunOptions};

#[derive(Clone)]
pub(super) struct PreparedEvalOptions {
    pub(super) baseline: Option<EvalReport>,
    pub(super) threshold_options: EvalThresholdOptions,
    pub(super) trend_path: Option<std::path::PathBuf>,
}

pub(super) fn prepare_eval_options(options: &EvalRunOptions) -> Result<PreparedEvalOptions> {
    let baseline = match options.baseline_report.as_deref() {
        Some(path) => Some(load_eval_report(path)?),
        None => None,
    };
    let min_rule_thresholds = parse_rule_threshold_args(&options.min_rule_f1, "min-rule-f1")?;
    let max_rule_drop_thresholds =
        parse_rule_threshold_args(&options.max_rule_f1_drop, "max-rule-f1-drop")?;

    Ok(PreparedEvalOptions {
        baseline,
        threshold_options: EvalThresholdOptions {
            max_micro_f1_drop: options.max_micro_f1_drop,
            max_suite_f1_drop: options.max_suite_f1_drop,
            max_category_f1_drop: options.max_category_f1_drop,
            max_language_f1_drop: options.max_language_f1_drop,
            min_micro_f1: options.min_micro_f1,
            min_macro_f1: options.min_macro_f1,
            min_rule_f1: min_rule_thresholds,
            max_rule_f1_drop: max_rule_drop_thresholds,
        },
        trend_path: options.trend_file.clone(),
    })
}
