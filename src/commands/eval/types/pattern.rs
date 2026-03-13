use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalExpectations {
    #[serde(default)]
    pub(in super::super) must_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(in super::super) must_not_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(in super::super) min_total: Option<usize>,
    #[serde(default)]
    pub(in super::super) max_total: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalPattern {
    #[serde(default)]
    pub(in super::super) file: Option<String>,
    #[serde(default)]
    pub(in super::super) line: Option<usize>,
    #[serde(default)]
    pub(in super::super) contains: Option<String>,
    #[serde(default)]
    pub(in super::super) contains_any: Vec<String>,
    #[serde(default)]
    pub(in super::super) matches_regex: Option<String>,
    #[serde(default)]
    pub(in super::super) severity: Option<String>,
    #[serde(default)]
    pub(in super::super) category: Option<String>,
    #[serde(default)]
    pub(in super::super) tags_any: Vec<String>,
    #[serde(default)]
    pub(in super::super) confidence_at_least: Option<f32>,
    #[serde(default)]
    pub(in super::super) confidence_at_most: Option<f32>,
    #[serde(default)]
    pub(in super::super) fix_effort: Option<String>,
    #[serde(default)]
    pub(in super::super) rule_id: Option<String>,
    #[serde(default)]
    pub(in super::super) require_rule_id: bool,
}
