use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct EvalRunOptions {
    pub baseline_report: Option<PathBuf>,
    pub max_micro_f1_drop: Option<f32>,
    pub min_micro_f1: Option<f32>,
    pub min_macro_f1: Option<f32>,
    pub min_rule_f1: Vec<String>,
    pub max_rule_f1_drop: Vec<String>,
    pub suite_filters: Vec<String>,
    pub category_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub fixture_name_filters: Vec<String>,
    pub max_fixtures: Option<usize>,
    pub label: Option<String>,
}
