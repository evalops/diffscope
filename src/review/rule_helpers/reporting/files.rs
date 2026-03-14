use std::collections::HashMap;

use crate::core;

pub fn severity_rank(severity: &core::comment::Severity) -> usize {
    match severity {
        core::comment::Severity::Error => 0,
        core::comment::Severity::Warning => 1,
        core::comment::Severity::Info => 2,
        core::comment::Severity::Suggestion => 3,
    }
}

pub fn format_top_findings_by_file(
    comments: &[core::Comment],
    max_files: usize,
    per_file: usize,
) -> String {
    if comments.is_empty() || max_files == 0 || per_file == 0 {
        return "- None\n".to_string();
    }

    let mut grouped: HashMap<String, Vec<&core::Comment>> = HashMap::new();
    for comment in comments {
        grouped
            .entry(comment.file_path.display().to_string())
            .or_default()
            .push(comment);
    }

    for file_comments in grouped.values_mut() {
        file_comments.sort_by(|left, right| {
            severity_rank(&left.severity)
                .cmp(&severity_rank(&right.severity))
                .then_with(|| left.line_number.cmp(&right.line_number))
        });
    }

    let mut rows = grouped.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .1
            .len()
            .cmp(&left.1.len())
            .then_with(|| left.0.cmp(&right.0))
    });
    rows.truncate(max_files);

    let mut out = String::new();
    for (path, file_comments) in rows {
        out.push_str(&format!(
            "- `{}` ({} issue(s))\n",
            path,
            file_comments.len()
        ));
        for comment in file_comments.into_iter().take(per_file) {
            let rule = comment
                .rule_id
                .as_deref()
                .map(|rule_id| format!(" rule:{rule_id}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  - `L{}` [{:?}{}] {}\n",
                comment.line_number, comment.severity, rule, comment.content
            ));
        }
    }
    out
}
