use anyhow::Result;
use std::collections::HashSet;

use crate::config::{self, ModelRole};

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

pub(super) fn ensure_frontier_eval_models(
    config: &config::Config,
    options: &EvalRunOptions,
) -> Result<()> {
    if options.allow_subfrontier_models {
        return Ok(());
    }

    let mut models = Vec::new();
    let mut seen_models = HashSet::new();
    for model in std::iter::once(config.model.clone())
        .chain(options.matrix_models.iter().cloned())
        .chain(
            std::iter::once(config.verification.model_role)
                .chain(config.verification.additional_model_roles.iter().copied())
                .map(|role| config.model_for_role(role).to_string()),
        )
        .chain(
            options
                .repro_validate
                .then(|| config.model_for_role(ModelRole::Fast).to_string()),
        )
    {
        if seen_models.insert(model.clone()) {
            models.push(model);
        }
    }

    let subfrontier_models = models
        .into_iter()
        .filter(|model| !is_frontier_review_model(model))
        .collect::<Vec<_>>();
    if !subfrontier_models.is_empty() {
        anyhow::bail!(
            "Eval requires frontier-grade review/judge models by default; set stronger models or pass --allow-subfrontier-models. Rejected model(s): {}",
            subfrontier_models.join(", ")
        );
    }

    Ok(())
}

fn is_frontier_review_model(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    normalized.contains("claude-opus-4.5")
        || normalized.contains("claude-opus-4-5")
        || normalized.contains("claude-opus-4.6")
        || normalized.contains("claude-opus-4-6")
        || normalized.contains("claude-sonnet-4.5")
        || normalized.contains("claude-sonnet-4-5")
        || normalized.contains("claude-sonnet-4.6")
        || normalized.contains("claude-sonnet-4-6")
        || normalized.starts_with("gpt-5")
        || normalized.starts_with("o3")
        || normalized.starts_with("o4")
        || normalized.contains("gemini-2.5-pro")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_frontier_review_model_accepts_requested_defaults() {
        assert!(is_frontier_review_model("anthropic/claude-opus-4.5"));
        assert!(is_frontier_review_model("anthropic/claude-sonnet-4.5"));
    }

    #[test]
    fn is_frontier_review_model_rejects_small_models() {
        assert!(!is_frontier_review_model("claude-haiku-4-5"));
        assert!(!is_frontier_review_model("gpt-4o-mini"));
        assert!(!is_frontier_review_model("anthropic/claude-opus-4.1"));
    }
}
