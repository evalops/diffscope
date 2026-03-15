use anyhow::Result;
use std::path::PathBuf;

use crate::core;
use crate::review::summarize_rule_hits;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Patch,
    Markdown,
}

pub async fn output_comments(
    comments: &[core::Comment],
    output_path: Option<PathBuf>,
    format: OutputFormat,
    rule_priority: &[String],
) -> Result<()> {
    let output = match format {
        OutputFormat::Json => serde_json::to_string_pretty(comments)?,
        OutputFormat::Patch => format_as_patch(comments),
        OutputFormat::Markdown => format_as_markdown(comments, rule_priority),
    };

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
    } else {
        println!("{output}");
    }

    Ok(())
}

fn format_completeness(summary: &core::comment::ReviewSummary) -> String {
    format!(
        "{} acknowledged · {} fixed · {} stale",
        summary.completeness.acknowledged_findings,
        summary.completeness.fixed_findings,
        summary.completeness.stale_findings
    )
}

pub fn format_as_patch(comments: &[core::Comment]) -> String {
    let mut output = String::new();
    for comment in comments {
        output.push_str(&format!(
            "# {}:{} - {:?}\n# {}\n",
            comment.file_path.display(),
            comment.line_number,
            comment.severity,
            comment.content
        ));
        if let Some(rule_id) = &comment.rule_id {
            output.push_str(&format!("# Rule: {rule_id}\n"));
        }
        if let Some(suggestion) = &comment.suggestion {
            output.push_str(&format!("# Suggestion: {suggestion}\n"));
        }
        if let Some(code_suggestion) = &comment.code_suggestion {
            output.push_str(&format!("# Code fix:\n{}\n", code_suggestion.diff));
        }
    }
    output
}

pub fn format_as_markdown(comments: &[core::Comment], rule_priority: &[String]) -> String {
    let mut output = String::new();

    // Generate summary
    let summary = core::CommentSynthesizer::generate_summary(comments);

    output.push_str("# Code Review Results\n\n");
    output.push_str("## Summary\n\n");
    output.push_str(&format!(
        "📊 **Overall Score:** {:.1}/10\n",
        summary.overall_score
    ));
    output.push_str(&format!(
        "📝 **Total Issues:** {}\n",
        summary.total_comments
    ));
    output.push_str(&format!(
        "🚨 **Critical Issues:** {}\n",
        summary.critical_issues
    ));
    output.push_str(&format!(
        "📁 **Files Reviewed:** {}\n\n",
        summary.files_reviewed
    ));
    output.push_str(&format!(
        "🚦 **Merge Readiness:** {}\n",
        summary.merge_readiness
    ));
    output.push_str(&format!(
        "📌 **Lifecycle:** {} open · {} resolved · {} dismissed\n\n",
        summary.open_comments, summary.resolved_comments, summary.dismissed_comments
    ));
    output.push_str(&format!(
        "📎 **Completeness:** {}\n\n",
        format_completeness(&summary)
    ));
    output.push_str(&format!(
        "⛔ **Open Blockers:** {}\n\n",
        summary.open_blockers
    ));
    output.push_str(&format!(
        "🚧 **Blocking Open:** {} | 💡 **Informational Open:** {}\n\n",
        summary.open_blocking_comments, summary.open_informational_comments
    ));
    output.push_str(&format!(
        "🧪 **Verification:** {}",
        summary.verification.state
    ));
    if summary.verification.judge_count > 0 {
        output.push_str(&format!(
            " (votes {}/{}, warnings {})",
            summary.verification.required_votes,
            summary.verification.judge_count,
            summary.verification.warning_count
        ));
    }
    output.push_str("\n\n");
    if !summary.readiness_reasons.is_empty() {
        output.push_str("### Review State\n\n");
        for reason in &summary.readiness_reasons {
            output.push_str(&format!("- {}\n", reason));
        }
        output.push('\n');
    }

    // Severity breakdown
    output.push_str("### Issues by Severity\n\n");
    let severity_order = ["Error", "Warning", "Info", "Suggestion"];
    for severity in severity_order {
        let count = summary.by_severity.get(severity).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let emoji = match severity {
            "Error" => "🔴",
            "Warning" => "🟡",
            "Info" => "🔵",
            "Suggestion" => "💡",
            _ => "⚪",
        };
        output.push_str(&format!("{emoji} **{severity}:** {count}\n"));
    }
    output.push('\n');

    // Category breakdown
    output.push_str("### Issues by Category\n\n");
    let category_order = [
        "Security",
        "Performance",
        "Bug",
        "Maintainability",
        "Testing",
        "Style",
        "Documentation",
        "Architecture",
        "BestPractice",
    ];
    for category in category_order {
        let count = summary.by_category.get(category).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let emoji = match category {
            "Security" => "🔒",
            "Performance" => "⚡",
            "Bug" => "🐛",
            "Style" => "🎨",
            "Documentation" => "📚",
            "Testing" => "🧪",
            "Maintainability" => "🔧",
            "Architecture" => "🏗️",
            _ => "💭",
        };
        output.push_str(&format!("{emoji} **{category}:** {count}\n"));
    }
    output.push('\n');

    let rule_hits = summarize_rule_hits(comments, 12, rule_priority);
    if !rule_hits.is_empty() {
        output.push_str("### Issues by Rule\n\n");
        output.push_str("| Rule | Count | Error | Warning | Info | Suggestion |\n");
        output.push_str("|------|-------|-------|---------|------|------------|\n");
        for (rule_id, hit) in rule_hits {
            output.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} |\n",
                rule_id, hit.total, hit.errors, hit.warnings, hit.infos, hit.suggestions
            ));
        }
        output.push('\n');
    }

    // Recommendations
    if !summary.recommendations.is_empty() {
        output.push_str("### Recommendations\n\n");
        for rec in &summary.recommendations {
            output.push_str(&format!("- {rec}\n"));
        }
        output.push('\n');
    }

    output.push_str("---\n\n## Detailed Issues\n\n");

    // Group comments by file
    let mut comments_by_file = std::collections::BTreeMap::new();
    for comment in comments {
        comments_by_file
            .entry(&comment.file_path)
            .or_insert_with(Vec::new)
            .push(comment);
    }

    for (file_path, file_comments) in comments_by_file {
        output.push_str(&format!("### {}\n\n", file_path.display()));

        for comment in file_comments {
            let severity_emoji = match comment.severity {
                core::comment::Severity::Error => "🔴",
                core::comment::Severity::Warning => "🟡",
                core::comment::Severity::Info => "🔵",
                core::comment::Severity::Suggestion => "💡",
            };

            let effort_badge = match comment.fix_effort {
                core::comment::FixEffort::Low => "🟢 Quick Fix",
                core::comment::FixEffort::Medium => "🟡 Moderate",
                core::comment::FixEffort::High => "🔴 Complex",
            };

            output.push_str(&format!(
                "#### Line {} {} {:?}\n\n",
                comment.line_number, severity_emoji, comment.category
            ));

            output.push_str(&format!(
                "**Confidence:** {:.0}%\n",
                comment.confidence * 100.0
            ));
            if let Some(rule_id) = &comment.rule_id {
                output.push_str(&format!("**Rule:** `{rule_id}`\n"));
            }
            output.push_str(&format!("**Fix Effort:** {effort_badge}\n\n"));

            output.push_str(&format!("{}\n\n", comment.content));

            if let Some(suggestion) = &comment.suggestion {
                output.push_str(&format!("💡 **Suggestion:** {suggestion}\n\n"));
            }

            if let Some(code_suggestion) = &comment.code_suggestion {
                output.push_str("**Code Suggestion:**\n");
                output.push_str(&format!("```diff\n{}\n```\n\n", code_suggestion.diff));
                output.push_str(&format!("_{}_ \n\n", code_suggestion.explanation));
            }

            if !comment.tags.is_empty() {
                output.push_str("**Tags:** ");
                for (i, tag) in comment.tags.iter().enumerate() {
                    if i > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&format!("`{tag}`"));
                }
                output.push_str("\n\n");
            }

            output.push_str("---\n\n");
        }
    }

    output
}

pub fn format_smart_review_output(
    comments: &[core::Comment],
    summary: &core::comment::ReviewSummary,
    pr_summary: Option<&core::pr_summary::PRSummary>,
    walkthrough: &str,
    rule_priority: &[String],
) -> String {
    let mut output = String::new();

    output.push_str("# 🤖 Smart Review Analysis Results\n\n");

    // Executive Summary
    output.push_str("## 📊 Executive Summary\n\n");
    let score_emoji = if summary.overall_score >= 8.0 {
        "🟢"
    } else if summary.overall_score >= 6.0 {
        "🟡"
    } else {
        "🔴"
    };
    output.push_str(&format!(
        "{} **Code Quality Score:** {:.1}/10\n",
        score_emoji, summary.overall_score
    ));
    output.push_str(&format!(
        "📝 **Total Issues Found:** {}\n",
        summary.total_comments
    ));
    output.push_str(&format!(
        "🚨 **Critical Issues:** {}\n",
        summary.critical_issues
    ));
    output.push_str(&format!(
        "📁 **Files Analyzed:** {}\n\n",
        summary.files_reviewed
    ));
    output.push_str(&format!(
        "🚦 **Merge Readiness:** {}\n",
        summary.merge_readiness
    ));
    output.push_str(&format!(
        "📌 **Lifecycle:** {} open · {} resolved · {} dismissed\n\n",
        summary.open_comments, summary.resolved_comments, summary.dismissed_comments
    ));
    output.push_str(&format!(
        "📎 **Completeness:** {}\n\n",
        format_completeness(summary)
    ));
    output.push_str(&format!(
        "⛔ **Open Blockers:** {}\n\n",
        summary.open_blockers
    ));
    output.push_str(&format!(
        "🚧 **Blocking Open:** {} | 💡 **Informational Open:** {}\n\n",
        summary.open_blocking_comments, summary.open_informational_comments
    ));
    output.push_str(&format!(
        "🧪 **Verification:** {}",
        summary.verification.state
    ));
    if summary.verification.judge_count > 0 {
        output.push_str(&format!(
            " (votes {}/{}, warnings {})",
            summary.verification.required_votes,
            summary.verification.judge_count,
            summary.verification.warning_count
        ));
    }
    output.push_str("\n\n");
    if !summary.readiness_reasons.is_empty() {
        output.push_str("### 🔁 Review State\n\n");
        for reason in &summary.readiness_reasons {
            output.push_str(&format!("- {}\n", reason));
        }
        output.push('\n');
    }

    if let Some(pr_summary) = pr_summary {
        output.push_str(&format_pr_summary_section(pr_summary));
        output.push('\n');
    }

    if !walkthrough.trim().is_empty() {
        output.push_str(walkthrough);
        output.push('\n');
    }

    // Quick Stats
    output.push_str("### 📈 Issue Breakdown\n\n");

    output.push_str("#### By Severity\n\n");
    output.push_str("| Severity | Count |\n");
    output.push_str("|----------|-------|\n");
    let severities = ["Error", "Warning", "Info", "Suggestion"];
    for severity in severities {
        let sev_count = summary.by_severity.get(severity).unwrap_or(&0);
        output.push_str(&format!("| {severity} | {sev_count} |\n"));
    }
    output.push('\n');

    output.push_str("#### By Category\n\n");
    output.push_str("| Category | Count |\n");
    output.push_str("|----------|-------|\n");
    let categories = [
        "Security",
        "Performance",
        "Bug",
        "Maintainability",
        "Testing",
        "Style",
        "Documentation",
        "Architecture",
        "BestPractice",
    ];
    for category in categories {
        let cat_count = summary.by_category.get(category).unwrap_or(&0);
        output.push_str(&format!("| {category} | {cat_count} |\n"));
    }
    output.push('\n');

    let rule_hits = summarize_rule_hits(comments, 12, rule_priority);
    if !rule_hits.is_empty() {
        output.push_str("#### By Rule\n\n");
        output.push_str("| Rule | Count | Error | Warning | Info | Suggestion |\n");
        output.push_str("|------|-------|-------|---------|------|------------|\n");
        for (rule_id, hit) in rule_hits {
            output.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} |\n",
                rule_id, hit.total, hit.errors, hit.warnings, hit.infos, hit.suggestions
            ));
        }
        output.push('\n');
    }

    // Actionable Recommendations
    if !summary.recommendations.is_empty() {
        output.push_str("### 🎯 Priority Actions\n\n");
        for (i, rec) in summary.recommendations.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, rec));
        }
        output.push('\n');
    }

    if comments.is_empty() {
        output.push_str("✅ **No issues found!** Your code looks good.\n");
        return output;
    }

    output.push_str("---\n\n## 🔍 Detailed Analysis\n\n");

    // Group by severity for better organization
    let mut critical_issues = Vec::new();
    let mut high_issues = Vec::new();
    let mut medium_issues = Vec::new();
    let mut low_issues = Vec::new();

    for comment in comments {
        match comment.severity {
            core::comment::Severity::Error => critical_issues.push(comment),
            core::comment::Severity::Warning => high_issues.push(comment),
            core::comment::Severity::Info => medium_issues.push(comment),
            core::comment::Severity::Suggestion => low_issues.push(comment),
        }
    }

    // Output each severity group
    if !critical_issues.is_empty() {
        output.push_str("### 🔴 Critical Issues (Fix Immediately)\n\n");
        for comment in critical_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !high_issues.is_empty() {
        output.push_str("### 🟡 High Priority Issues\n\n");
        for comment in high_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !medium_issues.is_empty() {
        output.push_str("### 🔵 Medium Priority Issues\n\n");
        for comment in medium_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    if !low_issues.is_empty() {
        output.push_str("### 💡 Suggestions & Improvements\n\n");
        for comment in low_issues {
            output.push_str(&format_detailed_comment(comment));
        }
    }

    output
}

pub fn format_detailed_comment(comment: &core::Comment) -> String {
    let mut output = String::new();

    let category_emoji = match comment.category {
        core::comment::Category::Security => "🔒",
        core::comment::Category::Performance => "⚡",
        core::comment::Category::Bug => "🐛",
        core::comment::Category::Style => "🎨",
        core::comment::Category::Documentation => "📚",
        core::comment::Category::Testing => "🧪",
        core::comment::Category::Maintainability => "🔧",
        core::comment::Category::Architecture => "🏗️",
        _ => "💭",
    };

    let effort_badge = match comment.fix_effort {
        core::comment::FixEffort::Low => "🟢 Quick Fix",
        core::comment::FixEffort::Medium => "🟡 Moderate Effort",
        core::comment::FixEffort::High => "🔴 Significant Effort",
    };

    output.push_str(&format!(
        "#### {} **{}:{}** - {} {:?}\n\n",
        category_emoji,
        comment.file_path.display(),
        comment.line_number,
        effort_badge,
        comment.category
    ));

    if comment.tags.is_empty() {
        output.push_str(&format!(
            "**Confidence:** {:.0}%\n\n",
            comment.confidence * 100.0
        ));
    } else {
        output.push_str(&format!(
            "**Confidence:** {:.0}% | **Tags:** ",
            comment.confidence * 100.0
        ));
        for (i, tag) in comment.tags.iter().enumerate() {
            if i > 0 {
                output.push_str(", ");
            }
            output.push_str(&format!("`{tag}`"));
        }
        output.push_str("\n\n");
    }
    if let Some(rule_id) = &comment.rule_id {
        output.push_str(&format!("**Rule:** `{rule_id}`\n\n"));
    }

    output.push_str(&format!("{}\n\n", comment.content));

    if let Some(suggestion) = &comment.suggestion {
        output.push_str(&format!("**💡 Recommended Fix:**\n{suggestion}\n\n"));
    }

    if let Some(code_suggestion) = &comment.code_suggestion {
        output.push_str("**🔧 Code Example:**\n");
        output.push_str(&format!("```diff\n{}\n```\n", code_suggestion.diff));
        output.push_str(&format!("_{}_\n\n", code_suggestion.explanation));
    }

    output.push_str("---\n\n");
    output
}

pub fn format_pr_summary_section(summary: &core::pr_summary::PRSummary) -> String {
    let mut output = String::new();
    output.push_str("## 🧾 PR Summary\n\n");
    output.push_str(&format!(
        "**{}** ({:?})\n\n",
        summary.title, summary.change_type
    ));

    if !summary.description.is_empty() {
        output.push_str(&format!("{}\n\n", summary.description));
    }

    if !summary.key_changes.is_empty() {
        output.push_str("### Key Changes\n\n");
        for change in &summary.key_changes {
            output.push_str(&format!("- {change}\n"));
        }
        output.push('\n');
    }

    if let Some(breaking) = &summary.breaking_changes {
        output.push_str("### Breaking Changes\n\n");
        output.push_str(&format!("{breaking}\n\n"));
    }

    if !summary.testing_notes.is_empty() {
        output.push_str("### Testing Notes\n\n");
        output.push_str(&format!("{}\n\n", summary.testing_notes));
    }

    if let Some(diagram) = &summary.visual_diff {
        if !diagram.trim().is_empty() {
            output.push_str("### Diagram\n\n");
            output.push_str("```mermaid\n");
            output.push_str(diagram.trim());
            output.push_str("\n```\n\n");
        }
    }

    output
}

pub fn format_diff_as_unified(diff: &core::UnifiedDiff) -> String {
    let mut output = String::new();

    for hunk in &diff.hunks {
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));

        for line in &hunk.changes {
            let prefix = match line.change_type {
                core::diff_parser::ChangeType::Added => "+",
                core::diff_parser::ChangeType::Removed => "-",
                core::diff_parser::ChangeType::Context => " ",
            };
            output.push_str(&format!("{}{}\n", prefix, line.content));
        }
    }

    output
}

pub fn build_change_walkthrough(diffs: &[core::UnifiedDiff]) -> String {
    let mut entries = Vec::new();
    let mut truncated = false;
    let max_entries = 50usize;

    for diff in diffs {
        if diff.is_binary {
            continue;
        }

        let mut added = 0usize;
        let mut removed = 0usize;
        for hunk in &diff.hunks {
            for change in &hunk.changes {
                match change.change_type {
                    core::diff_parser::ChangeType::Added => added += 1,
                    core::diff_parser::ChangeType::Removed => removed += 1,
                    _ => {}
                }
            }
        }

        let status = if diff.is_deleted {
            "deleted"
        } else if diff.is_new {
            "new"
        } else {
            "modified"
        };

        if entries.len() >= max_entries {
            truncated = true;
            break;
        }

        entries.push(format!(
            "- `{}` ({}; +{}, -{})",
            diff.file_path.display(),
            status,
            added,
            removed
        ));
    }

    if entries.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("## 🧭 Change Walkthrough\n\n");
    output.push_str(&entries.join("\n"));
    output.push('\n');
    if truncated {
        output.push_str("\n...truncated (too many files)\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn build_test_comment(
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
            resolved_at: None,
        }
    }

    #[test]
    fn format_patch_includes_rule_id() {
        let mut comment = build_test_comment(
            "rule-patch",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        comment.rule_id = Some("sec.auth.guard".to_string());
        let patch = format_as_patch(&[comment]);
        assert!(patch.contains("# Rule: sec.auth.guard"));
    }

    #[test]
    fn format_patch_includes_suggestion() {
        let mut comment = build_test_comment(
            "suggest",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.9,
        );
        comment.suggestion = Some("Use safe API".to_string());
        let patch = format_as_patch(&[comment]);
        assert!(patch.contains("# Suggestion: Use safe API"));
    }

    #[test]
    fn format_patch_basic_structure() {
        let comment = build_test_comment(
            "basic",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        let patch = format_as_patch(&[comment]);
        assert!(patch.contains("src/lib.rs:10"));
        assert!(patch.contains("Error"));
        assert!(patch.contains("test comment"));
    }

    #[test]
    fn format_markdown_includes_summary() {
        let comments = vec![build_test_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        )];
        let md = format_as_markdown(&comments, &[]);
        assert!(md.contains("# Code Review Results"));
        assert!(md.contains("Overall Score"));
        assert!(md.contains("Total Issues"));
    }

    #[test]
    fn format_markdown_includes_severity_breakdown() {
        let comments = vec![build_test_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        )];
        let md = format_as_markdown(&comments, &[]);
        assert!(md.contains("Issues by Severity"));
        assert!(md.contains("🔴"));
    }

    #[test]
    fn format_markdown_includes_rule_table() {
        let mut comment = build_test_comment(
            "c1",
            core::comment::Category::Security,
            core::comment::Severity::Warning,
            0.9,
        );
        comment.rule_id = Some("sec.xss".to_string());
        let md = format_as_markdown(&[comment], &[]);
        assert!(md.contains("Issues by Rule"));
        assert!(md.contains("sec.xss"));
    }

    #[test]
    fn format_markdown_empty_comments() {
        let md = format_as_markdown(&[], &[]);
        assert!(md.contains("# Code Review Results"));
        assert!(md.contains("Total Issues:** 0"));
    }

    #[test]
    fn format_detailed_comment_has_structure() {
        let mut comment = build_test_comment(
            "det",
            core::comment::Category::Security,
            core::comment::Severity::Error,
            0.85,
        );
        comment.rule_id = Some("sec.auth".to_string());
        comment.suggestion = Some("Add auth check".to_string());
        comment.tags = vec!["auth".to_string()];

        let output = format_detailed_comment(&comment);
        assert!(output.contains("🔒"));
        assert!(output.contains("sec.auth"));
        assert!(output.contains("Recommended Fix"));
        assert!(output.contains("auth"));
        assert!(output.contains("85%"));
    }

    #[test]
    fn format_diff_as_unified_produces_output() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 3,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "unchanged".to_string(),
                        change_type: core::diff_parser::ChangeType::Context,
                        old_line_no: Some(1),
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "removed".to_string(),
                        change_type: core::diff_parser::ChangeType::Removed,
                        old_line_no: Some(2),
                        new_line_no: None,
                    },
                    core::diff_parser::DiffLine {
                        content: "added".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                ],
            }],
        };
        let output = format_diff_as_unified(&diff);
        assert!(output.contains("@@ -1,3 +1,3 @@"));
        assert!(output.contains("+added"));
        assert!(output.contains("-removed"));
        assert!(output.contains(" unchanged"));
    }

    #[test]
    fn build_change_walkthrough_basic() {
        let diffs = vec![core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("src/main.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![core::diff_parser::DiffHunk {
                context: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                changes: vec![
                    core::diff_parser::DiffLine {
                        content: "old".to_string(),
                        change_type: core::diff_parser::ChangeType::Removed,
                        old_line_no: Some(1),
                        new_line_no: None,
                    },
                    core::diff_parser::DiffLine {
                        content: "new1".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    core::diff_parser::DiffLine {
                        content: "new2".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                ],
            }],
        }];
        let walkthrough = build_change_walkthrough(&diffs);
        assert!(walkthrough.contains("src/main.rs"));
        assert!(walkthrough.contains("modified"));
        assert!(walkthrough.contains("+2"));
        assert!(walkthrough.contains("-1"));
    }

    #[test]
    fn build_change_walkthrough_empty_diffs() {
        let walkthrough = build_change_walkthrough(&[]);
        assert!(walkthrough.is_empty());
    }

    #[test]
    fn build_change_walkthrough_skips_binary() {
        let diffs = vec![core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("image.png"),
            is_new: false,
            is_deleted: false,
            is_binary: true,
            hunks: vec![],
        }];
        let walkthrough = build_change_walkthrough(&diffs);
        assert!(walkthrough.is_empty());
    }

    #[test]
    fn smart_review_output_has_executive_summary() {
        let summary = core::comment::ReviewSummary {
            total_comments: 2,
            critical_issues: 1,
            files_reviewed: 1,
            overall_score: 7.5,
            by_severity: std::collections::HashMap::from([
                ("Error".to_string(), 1),
                ("Warning".to_string(), 1),
            ]),
            by_category: std::collections::HashMap::from([("Bug".to_string(), 2)]),
            recommendations: vec!["Fix bugs".to_string()],
            open_comments: 2,
            open_by_severity: std::collections::HashMap::from([
                ("Error".to_string(), 1),
                ("Warning".to_string(), 1),
            ]),
            open_blocking_comments: 2,
            open_informational_comments: 0,
            resolved_comments: 0,
            dismissed_comments: 0,
            open_blockers: 2,
            completeness: crate::core::comment::ReviewCompletenessSummary {
                total_findings: 2,
                acknowledged_findings: 0,
                fixed_findings: 0,
                stale_findings: 0,
            },
            merge_readiness: crate::core::comment::MergeReadiness::NeedsAttention,
            verification: crate::core::comment::ReviewVerificationSummary::default(),
            readiness_reasons: Vec::new(),
            loop_telemetry: None,
        };
        let comments = vec![
            build_test_comment(
                "c1",
                core::comment::Category::Bug,
                core::comment::Severity::Error,
                0.9,
            ),
            build_test_comment(
                "c2",
                core::comment::Category::Bug,
                core::comment::Severity::Warning,
                0.8,
            ),
        ];
        let output = format_smart_review_output(&comments, &summary, None, "", &[]);
        assert!(output.contains("Smart Review Analysis"));
        assert!(output.contains("Executive Summary"));
        assert!(output.contains("7.5/10"));
        assert!(output.contains("Critical Issues"));
        assert!(output.contains("Completeness"));
        assert!(output.contains("Fix bugs"));
    }

    #[test]
    fn format_markdown_file_sections_are_deterministic() {
        // Comments across multiple files should produce deterministic file section ordering
        let mut c1 = build_test_comment(
            "c1",
            core::comment::Category::Bug,
            core::comment::Severity::Error,
            0.9,
        );
        c1.file_path = PathBuf::from("src/z_last.rs");

        let mut c2 = build_test_comment(
            "c2",
            core::comment::Category::Bug,
            core::comment::Severity::Warning,
            0.8,
        );
        c2.file_path = PathBuf::from("src/a_first.rs");

        let mut c3 = build_test_comment(
            "c3",
            core::comment::Category::Security,
            core::comment::Severity::Info,
            0.7,
        );
        c3.file_path = PathBuf::from("src/m_middle.rs");

        let comments = vec![c1, c2, c3];
        let output = format_as_markdown(&comments, &[]);

        // Run multiple times — should always produce the same output
        for _ in 0..5 {
            let output2 = format_as_markdown(&comments, &[]);
            assert_eq!(
                output, output2,
                "Markdown output should be deterministic across runs"
            );
        }

        // File sections should appear in sorted order
        let z_pos = output
            .find("src/z_last.rs")
            .expect("should contain z_last.rs");
        let a_pos = output
            .find("src/a_first.rs")
            .expect("should contain a_first.rs");
        let m_pos = output
            .find("src/m_middle.rs")
            .expect("should contain m_middle.rs");
        assert!(
            a_pos < m_pos && m_pos < z_pos,
            "Files should appear in sorted order: a_first({a_pos}) < m_middle({m_pos}) < z_last({z_pos})"
        );
    }

    // ── Bug: walkthrough falsely reports truncation at exactly max_entries ──
    //
    // The truncation check ran after pushing the entry, so when exactly
    // max_entries (50) files exist, it would say "truncated" even though
    // all entries were included.

    #[test]
    fn test_walkthrough_not_truncated_at_exactly_max() {
        let diffs: Vec<core::UnifiedDiff> = (0..50)
            .map(|i| core::UnifiedDiff {
                file_path: PathBuf::from(format!("file{i}.rs")),
                old_content: None,
                new_content: None,
                is_new: false,
                is_deleted: false,
                is_binary: false,
                hunks: vec![core::diff_parser::DiffHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    context: String::new(),
                    changes: vec![core::diff_parser::DiffLine {
                        content: "line".to_string(),
                        change_type: core::diff_parser::ChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    }],
                }],
            })
            .collect();

        let output = build_change_walkthrough(&diffs);
        assert!(
            !output.contains("truncated"),
            "50 files (exactly max_entries) should not be truncated"
        );
        // All 50 files should be present
        for i in 0..50 {
            assert!(
                output.contains(&format!("file{i}.rs")),
                "Missing file{i}.rs in walkthrough"
            );
        }
    }
}
