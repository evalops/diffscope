use crate::core;

use super::files::format_top_findings_by_file;
use super::rules::summarize_rule_hits;

pub fn build_pr_summary_comment_body(
    comments: &[core::Comment],
    rule_priority: &[String],
) -> String {
    let summary = core::CommentSynthesizer::generate_summary(comments);
    let mut body = String::new();
    body.push_str("## DiffScope Review Summary\n\n");
    body.push_str(&format!("- Total issues: {}\n", summary.total_comments));
    body.push_str(&format!("- Critical issues: {}\n", summary.critical_issues));
    body.push_str(&format!("- Files reviewed: {}\n", summary.files_reviewed));
    body.push_str(&format!(
        "- Overall score: {:.1}/10\n",
        summary.overall_score
    ));
    body.push_str(&format!("- Merge readiness: {}\n", summary.merge_readiness));
    body.push_str(&format!(
        "- Lifecycle: {} open / {} resolved / {} dismissed\n",
        summary.open_comments, summary.resolved_comments, summary.dismissed_comments
    ));
    body.push_str(&format!("- Open blockers: {}\n", summary.open_blockers));
    body.push_str(&format!(
        "- Blocking open: {} | Informational open: {}\n",
        summary.open_blocking_comments, summary.open_informational_comments
    ));
    body.push_str(&format!("- Verification: {}", summary.verification.state));
    if summary.verification.judge_count > 0 {
        body.push_str(&format!(
            " (votes {}/{}, warnings {})",
            summary.verification.required_votes,
            summary.verification.judge_count,
            summary.verification.warning_count
        ));
    }
    body.push('\n');
    if !summary.readiness_reasons.is_empty() {
        body.push_str("- Review state:\n");
        for reason in &summary.readiness_reasons {
            body.push_str(&format!("  - {}\n", reason));
        }
    }

    if summary.total_comments == 0 {
        body.push_str("\nNo issues detected in this PR by DiffScope.\n");
        return body;
    }

    body.push_str("\n### Severity Breakdown\n");
    for severity in ["Error", "Warning", "Info", "Suggestion"] {
        let count = summary.by_severity.get(severity).copied().unwrap_or(0);
        body.push_str(&format!("- {}: {}\n", severity, count));
    }

    let rule_hits = summarize_rule_hits(comments, 8, rule_priority);
    if !rule_hits.is_empty() {
        body.push_str("\n### Rule Hits\n");
        for (rule_id, hit) in rule_hits {
            body.push_str(&format!(
                "- `{}`: {} hit(s) (E:{} W:{} I:{} S:{})\n",
                rule_id, hit.total, hit.errors, hit.warnings, hit.infos, hit.suggestions
            ));
        }
    }

    body.push_str("\n### Top Findings by File\n");
    body.push_str(&format_top_findings_by_file(comments, 5, 2));

    body
}
