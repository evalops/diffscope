use crate::core;
use crate::review::normalize_rule_id;

use super::super::super::EvalPattern;

impl EvalPattern {
    pub(in super::super::super) fn normalized_rule_id(&self) -> Option<String> {
        normalize_rule_id(self.rule_id.as_deref())
    }
}

pub(super) fn matches_rule_id_requirement(pattern: &EvalPattern, comment: &core::Comment) -> bool {
    if let Some(rule_id) = &pattern.rule_id {
        if pattern.require_rule_id {
            let expected = rule_id.trim().to_ascii_lowercase();
            let actual = comment
                .rule_id
                .as_deref()
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_default();
            return expected == actual;
        }
    }

    true
}
