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

    #[test]
    fn test_eval_pattern_matches_semantic_phrase_aliases_and_rule_id_aliases() {
        let comment = core::Comment {
            id: "comment-2".to_string(),
            file_path: PathBuf::from("src/auth.rs"),
            line_number: 18,
            content: "This introduces directory traversal and bypasses authorization checks."
                .to_string(),
            rule_id: Some("security.path-traversal".to_string()),
            severity: Severity::Error,
            category: Category::Security,
            suggestion: None,
            confidence: 0.88,
            code_suggestion: None,
            tags: vec!["path-traversal".to_string()],
            fix_effort: FixEffort::Medium,
            feedback: None,
        };

        let pattern = EvalPattern {
            contains_any: vec!["path traversal".to_string(), "authz bypass".to_string()],
            rule_id: Some("sec.path.traversal".to_string()),
            rule_id_aliases: vec!["security.path-traversal".to_string()],
            require_rule_id: true,
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn test_eval_pattern_matches_semantic_tag_aliases() {
        let comment = core::Comment {
            id: "comment-3".to_string(),
            file_path: PathBuf::from("admin.py"),
            line_number: 2,
            content: "Authorization bypass via query parameter lets any user delete accounts."
                .to_string(),
            rule_id: None,
            severity: Severity::Error,
            category: Category::Security,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec!["broken-access-control".to_string(), "cwe-284".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
        };

        let pattern = EvalPattern {
            tags_any: vec!["authorization".to_string()],
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn test_eval_pattern_matches_adjacent_line_hint() {
        let comment = core::Comment {
            id: "comment-4".to_string(),
            file_path: PathBuf::from("Dockerfile"),
            line_number: 5,
            content: "Piping a remote script to bash is a supply chain risk.".to_string(),
            rule_id: None,
            severity: Severity::Error,
            category: Category::Security,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec!["supply-chain".to_string()],
            fix_effort: FixEffort::Medium,
            feedback: None,
        };

        let pattern = EvalPattern {
            file: Some("Dockerfile".to_string()),
            line: Some(4),
            contains_any: vec!["supply chain risk".to_string()],
            severity: Some("error".to_string()),
            category: Some("security".to_string()),
            tags_any: vec!["supply-chain".to_string()],
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn test_eval_pattern_matches_supply_chain_phrase_variants() {
        let comment = core::Comment {
            id: "comment-5".to_string(),
            file_path: PathBuf::from("Dockerfile"),
            line_number: 5,
            content: "Piping remote script directly to bash executes arbitrary code without verification. Impact: Supply chain attack vector if the host is compromised."
                .to_string(),
            rule_id: None,
            severity: Severity::Error,
            category: Category::Security,
            suggestion: Some(
                "Download the script first, verify its checksum, then execute it."
                    .to_string(),
            ),
            confidence: 0.9,
            code_suggestion: None,
            tags: vec!["supply-chain".to_string()],
            fix_effort: FixEffort::Medium,
            feedback: None,
        };

        let pattern = EvalPattern {
            file: Some("Dockerfile".to_string()),
            line: Some(4),
            contains_any: vec![
                "remote script execution".to_string(),
                "verify downloaded script".to_string(),
                "supply chain risk".to_string(),
            ],
            severity: Some("error".to_string()),
            category: Some("security".to_string()),
            tags_any: vec!["supply-chain".to_string()],
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }

    #[test]
    fn test_eval_pattern_matches_curl_output_to_bash_variant() {
        let comment = core::Comment {
            id: "comment-6".to_string(),
            file_path: PathBuf::from("Dockerfile"),
            line_number: 5,
            content: "Piping curl output directly to bash executes untrusted code without verification. Impact: An attacker controlling the host can run arbitrary commands during the build."
                .to_string(),
            rule_id: Some("sec.supply-chain.curl-pipe-bash".to_string()),
            severity: Severity::Error,
            category: Category::Security,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec!["supply-chain".to_string(), "code-execution".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
        };

        let pattern = EvalPattern {
            file: Some("Dockerfile".to_string()),
            line: Some(4),
            contains_any: vec![
                "curl pipe to shell".to_string(),
                "remote script execution".to_string(),
                "verify downloaded script".to_string(),
            ],
            severity: Some("error".to_string()),
            category: Some("security".to_string()),
            tags_any: vec!["supply-chain".to_string()],
            ..Default::default()
        };

        assert!(pattern.matches(&comment));
    }
}
