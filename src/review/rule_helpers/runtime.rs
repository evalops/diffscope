#[path = "runtime/context.rs"]
mod context;
#[path = "runtime/overrides.rs"]
mod overrides;
#[path = "runtime/parsing.rs"]
mod parsing;

pub use context::inject_rule_context;
pub use overrides::apply_rule_overrides;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core;
    use std::path::PathBuf;

    fn build_comment(
        id: &str,
        category: core::comment::Category,
        severity: core::comment::Severity,
        confidence: f32,
    ) -> core::Comment {
        core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test comment".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        }
    }

    #[test]
    fn apply_rule_overrides_sets_severity_and_category() {
        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        );
        comment.rule_id = Some("sec.xss".to_string());

        let rules = vec![core::ReviewRule {
            source: String::new(),
            id: "sec.xss".to_string(),
            description: "XSS check".to_string(),
            severity: Some("error".to_string()),
            category: Some("security".to_string()),
            scope: None,
            tags: vec!["owasp".to_string()],
        }];

        let result = apply_rule_overrides(vec![comment], &rules);
        assert_eq!(result[0].severity, core::comment::Severity::Error);
        assert_eq!(result[0].category, core::comment::Category::Security);
        assert!(result[0].tags.contains(&"rule:sec.xss".to_string()));
        assert!(result[0].tags.contains(&"owasp".to_string()));
        assert!(result[0].confidence >= 0.8);
    }

    #[test]
    fn apply_rule_overrides_no_matching_rule() {
        let mut comment = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Info,
            0.5,
        );
        comment.rule_id = Some("other.rule".to_string());

        let rules = vec![core::ReviewRule {
            source: String::new(),
            id: "sec.xss".to_string(),
            description: "XSS check".to_string(),
            severity: None,
            category: None,
            scope: None,
            tags: vec![],
        }];

        let result = apply_rule_overrides(vec![comment], &rules);
        assert_eq!(result[0].severity, core::comment::Severity::Info);
    }
}
