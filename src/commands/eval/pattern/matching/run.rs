use crate::core;

use super::super::super::EvalPattern;
use super::predicates::{
    matches_category, matches_confidence_bounds, matches_contains, matches_contains_any,
    matches_file, matches_fix_effort, matches_line, matches_regex, matches_severity,
    matches_tags_any,
};
use super::rule_id::matches_rule_id_requirement;

impl EvalPattern {
    pub(in super::super::super) fn matches(&self, comment: &core::Comment) -> bool {
        if self.is_empty() {
            return false;
        }

        let content_lower = comment.content.to_ascii_lowercase();

        matches_file(self, comment)
            && matches_line(self, comment)
            && matches_contains(self, &content_lower)
            && matches_contains_any(self, &content_lower)
            && matches_tags_any(self, comment)
            && matches_regex(self, comment)
            && matches_severity(self, comment)
            && matches_category(self, comment)
            && matches_confidence_bounds(self, comment)
            && matches_fix_effort(self, comment)
            && matches_rule_id_requirement(self, comment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    #[test]
    fn test_eval_pattern_matches_regex_tags_and_confidence() {
        let comment = core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "Calling panic!(user_input) here can crash the request path".to_string(),
            rule_id: Some("panic.user-input".to_string()),
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: Some("Return an error instead of panicking".to_string()),
            confidence: 0.91,
            code_suggestion: None,
            tags: vec!["reliability".to_string(), "panic".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
        };

        let pattern = EvalPattern {
            contains_any: vec!["panic".to_string(), "unwrap".to_string()],
            matches_regex: Some("panic!\\([^)]*user_input[^)]*\\)".to_string()),
            tags_any: vec!["security".to_string(), "reliability".to_string()],
            confidence_at_least: Some(0.9),
            fix_effort: Some("low".to_string()),
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }
}
