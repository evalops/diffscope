use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct EvalRunOptions {
    pub baseline_report: Option<PathBuf>,
    pub max_micro_f1_drop: Option<f32>,
    pub max_suite_f1_drop: Option<f32>,
    pub max_category_f1_drop: Option<f32>,
    pub max_language_f1_drop: Option<f32>,
    pub min_micro_f1: Option<f32>,
    pub min_macro_f1: Option<f32>,
    pub min_verification_health: Option<f32>,
    pub min_rule_f1: Vec<String>,
    pub max_rule_f1_drop: Vec<String>,
    pub matrix_models: Vec<String>,
    pub repeat: usize,
    pub suite_filters: Vec<String>,
    pub category_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub fixture_name_filters: Vec<String>,
    pub max_fixtures: Option<usize>,
    pub label: Option<String>,
    pub trend_file: Option<PathBuf>,
    pub artifact_dir: Option<PathBuf>,
    pub allow_subfrontier_models: bool,
    pub repro_validate: bool,
    pub repro_max_comments: usize,
}
