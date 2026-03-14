#[path = "reporting/files.rs"]
mod files;
#[path = "reporting/rules.rs"]
mod rules;
#[path = "reporting/summary.rs"]
mod summary;

pub use files::{format_top_findings_by_file, severity_rank};
pub use rules::{
    build_rule_priority_rank, normalize_rule_id, summarize_rule_hits, RuleHitBreakdown,
};
pub use summary::build_pr_summary_comment_body;

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
            status: crate::core::comment::CommentStatus::Open,
        }
    }

    #[test]
    fn normalize_rule_id_trims_and_lowercases() {
        assert_eq!(
            normalize_rule_id(Some(" SEC.XSS ")),
            Some("sec.xss".to_string())
        );
        assert_eq!(normalize_rule_id(Some("")), None);
        assert_eq!(normalize_rule_id(None), None);
    }

    #[test]
    fn summarize_rule_hits_orders_by_volume() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        c3.rule_id = Some("rule.beta".to_string());

        let hits = summarize_rule_hits(&[c1, c2, c3], 8, &[]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "rule.alpha");
        assert_eq!(hits[0].1.total, 2);
        assert_eq!(hits[1].0, "rule.beta");
    }

    #[test]
    fn summarize_rule_hits_respects_priority_order() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c1.rule_id = Some("rule.alpha".to_string());
        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.rule_id = Some("rule.alpha".to_string());
        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.9,
        );
        c3.rule_id = Some("rule.beta".to_string());

        let hits = summarize_rule_hits(
            &[c1, c2, c3],
            8,
            &["rule.beta".to_string(), "rule.alpha".to_string()],
        );
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "rule.beta");
    }

    #[test]
    fn summarize_rule_hits_skips_comments_without_rules() {
        let c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        let hits = summarize_rule_hits(&[c1], 8, &[]);
        assert!(hits.is_empty());
    }

    #[test]
    fn top_findings_summary_groups_by_file() {
        let mut c1 = build_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.file_path = PathBuf::from("src/a.rs");
        c1.line_number = 11;
        c1.rule_id = Some("rule.alpha".to_string());

        let mut c2 = build_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        c2.file_path = PathBuf::from("src/a.rs");
        c2.line_number = 20;

        let mut c3 = build_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        c3.file_path = PathBuf::from("src/b.rs");
        c3.line_number = 5;

        let text = format_top_findings_by_file(&[c1, c2, c3], 5, 2);
        assert!(text.contains("`src/a.rs` (2 issue(s))"));
        assert!(text.contains("`L11` [Error rule:rule.alpha]"));
        assert!(text.contains("`src/b.rs` (1 issue(s))"));
    }

    #[test]
    fn top_findings_empty_comments() {
        let text = format_top_findings_by_file(&[], 5, 2);
        assert_eq!(text, "- None\n");
    }

    #[test]
    fn build_rule_priority_rank_basic() {
        let rank = build_rule_priority_rank(&["sec.xss".to_string(), "sec.sqli".to_string()]);
        assert_eq!(rank.get("sec.xss"), Some(&0));
        assert_eq!(rank.get("sec.sqli"), Some(&1));
    }

    #[test]
    fn build_rule_priority_rank_deduplicates() {
        let rank = build_rule_priority_rank(&["SEC.XSS".to_string(), "sec.xss".to_string()]);
        assert_eq!(rank.len(), 1);
        assert_eq!(rank.get("sec.xss"), Some(&0));
    }

    #[test]
    fn severity_rank_order() {
        assert!(
            severity_rank(&core::comment::Severity::Error)
                < severity_rank(&core::comment::Severity::Warning)
        );
        assert!(
            severity_rank(&core::comment::Severity::Warning)
                < severity_rank(&core::comment::Severity::Info)
        );
        assert!(
            severity_rank(&core::comment::Severity::Info)
                < severity_rank(&core::comment::Severity::Suggestion)
        );
    }

    #[test]
    fn pr_summary_body_includes_key_sections() {
        let mut c = build_comment(
            "c1",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.9,
        );
        c.rule_id = Some("sec.xss".to_string());
        let body = build_pr_summary_comment_body(&[c], &[]);
        assert!(body.contains("DiffScope Review Summary"));
        assert!(body.contains("Total issues: 1"));
        assert!(body.contains("sec.xss"));
    }

    #[test]
    fn pr_summary_body_empty_comments() {
        let body = build_pr_summary_comment_body(&[], &[]);
        assert!(body.contains("No issues detected"));
    }
}
