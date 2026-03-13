use crate::core;
use crate::review::normalize_rule_id;

use super::super::super::EvalPattern;

impl EvalPattern {
    pub(in super::super::super) fn normalized_rule_ids(&self) -> Vec<String> {
        let mut normalized = Vec::new();
        if let Some(rule_id) = normalize_rule_id(self.rule_id.as_deref()) {
            normalized.push(rule_id);
        }
        for alias in &self.rule_id_aliases {
            if let Some(alias) = normalize_rule_id(Some(alias.as_str())) {
                if !normalized.iter().any(|candidate| candidate == &alias) {
                    normalized.push(alias);
                }
            }
        }
        normalized
    }
}

pub(super) fn matches_rule_id_requirement(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    if pattern.require_rule_id {
        let expected_rule_ids = pattern.normalized_rule_ids();
        if expected_rule_ids.is_empty() {
            return false;
        }
        let actual = normalize_rule_id(comment.rule_id.as_deref()).unwrap_or_default();
        return expected_rule_ids.iter().any(|expected| expected == &actual);
    }

    true
}
