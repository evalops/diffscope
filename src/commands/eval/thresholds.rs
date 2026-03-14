#[path = "thresholds/evaluation.rs"]
mod evaluation;
#[path = "thresholds/parsing.rs"]
mod parsing;

#[derive(Debug, Clone)]
pub(super) struct EvalThresholdOptions {
    pub(super) max_micro_f1_drop: Option<f32>,
    pub(super) max_suite_f1_drop: Option<f32>,
    pub(super) max_category_f1_drop: Option<f32>,
    pub(super) max_language_f1_drop: Option<f32>,
    pub(super) min_micro_f1: Option<f32>,
    pub(super) min_macro_f1: Option<f32>,
    pub(super) min_verification_health: Option<f32>,
    pub(super) min_rule_f1: Vec<EvalRuleThreshold>,
    pub(super) max_rule_f1_drop: Vec<EvalRuleThreshold>,
}

#[derive(Debug, Clone)]
pub(super) struct EvalRuleThreshold {
    pub(super) rule_id: String,
    pub(super) value: f32,
}

pub(super) use evaluation::evaluate_eval_thresholds;
pub(super) use parsing::parse_rule_threshold_args;
