use crate::core::diff_parser::ChangeType;
use crate::core::UnifiedDiff;

pub(super) fn diff_snippet_for_comment(diff: &UnifiedDiff, line_number: usize) -> String {
    for hunk in &diff.hunks {
        let hunk_start = hunk.new_start;
        let hunk_end = hunk.new_start + hunk.new_lines.saturating_sub(1);
        if (hunk_start..=hunk_end.max(hunk_start)).contains(&line_number) {
            return hunk
                .changes
                .iter()
                .map(|change| {
                    let prefix = match change.change_type {
                        ChangeType::Added => "+",
                        ChangeType::Removed => "-",
                        ChangeType::Context => " ",
                    };
                    format!("{}{}", prefix, change.content)
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}

pub(super) fn source_context_for_line(content: &str, line_number: usize, radius: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let target_line = line_number.clamp(1, lines.len());
    let start = target_line.saturating_sub(radius + 1);
    let end = (target_line + radius).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{:>4}: {}", start + offset + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}
