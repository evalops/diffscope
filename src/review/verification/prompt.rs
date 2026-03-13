use std::collections::HashMap;

use crate::core::{Comment, ContextType, LLMContextChunk, UnifiedDiff};

pub(super) fn build_verification_prompt(
    comments: &[Comment],
    diffs: &[UnifiedDiff],
    source_files: &HashMap<std::path::PathBuf, String>,
    extra_context: &HashMap<std::path::PathBuf, Vec<LLMContextChunk>>,
) -> String {
    let diff_map = diffs
        .iter()
        .map(|diff| (diff.file_path.clone(), diff))
        .collect::<HashMap<_, _>>();

    let mut prompt = String::from("## Findings to Verify\n\n");

    for (i, comment) in comments.iter().enumerate() {
        let diff = diff_map.get(&comment.file_path).copied();
        prompt.push_str(&format!(
            "### Finding {}\n- File: {}:{}\n- Issue: {}\n",
            i + 1,
            comment.file_path.display(),
            comment.line_number,
            comment.content,
        ));
        if let Some(ref suggestion) = comment.suggestion {
            prompt.push_str(&format!("- Suggestion: {}\n", suggestion));
        }
        if let Some(diff) = diff {
            let diff_snippet = diff_snippet_for_comment(diff, comment.line_number);
            if !diff_snippet.trim().is_empty() {
                prompt.push_str("- Diff evidence:\n```diff\n");
                prompt.push_str(&diff_snippet);
                prompt.push_str("\n```\n");
            }
        }
        if let Some(content) = source_files.get(&comment.file_path) {
            let file_context = source_context_for_line(content, comment.line_number, 6);
            if !file_context.trim().is_empty() {
                prompt.push_str("- Nearby file context:\n```\n");
                prompt.push_str(&file_context);
                prompt.push_str("\n```\n");
            }
        }
        let supporting_context = supporting_context_for_comment(comment, extra_context);
        if !supporting_context.is_empty() {
            prompt.push_str("- Cross-file attachment rule: if this changed line introduces a risky call or tainted input into the helper below, the finding can still be accurate and line-correct even when the vulnerable sink lives in the supporting-context file.\n");
            prompt.push_str("- Supporting context:\n");
            for chunk in supporting_context {
                prompt.push_str("```text\n");
                prompt.push_str(&format_context_chunk_for_verification(&chunk));
                prompt.push_str("\n```\n");
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("Return JSON only. Do not add commentary outside the JSON array.\n");
    prompt
}

fn supporting_context_for_comment(
    comment: &Comment,
    extra_context: &HashMap<std::path::PathBuf, Vec<LLMContextChunk>>,
) -> Vec<LLMContextChunk> {
    let mut chunks = extra_context
        .get(&comment.file_path)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|chunk| {
            !(chunk.file_path == comment.file_path
                && chunk.context_type == ContextType::FileContent)
        })
        .collect::<Vec<_>>();

    chunks.sort_by_key(|chunk| std::cmp::Reverse(score_supporting_context(chunk, comment)));
    chunks.truncate(3);
    chunks
}

fn score_supporting_context(chunk: &LLMContextChunk, comment: &Comment) -> i32 {
    let mut score = match chunk.context_type {
        ContextType::Definition => 90,
        ContextType::Reference => 70,
        ContextType::Documentation => 45,
        ContextType::FileContent => 20,
    };

    if chunk.file_path != comment.file_path {
        score += 15;
    }

    if let Some(range) = chunk.line_range {
        if comment.line_number >= range.0 && comment.line_number <= range.1 {
            score += 10;
        }
    }

    if let Some(provenance) = chunk.provenance.as_ref() {
        score += provenance.verification_bonus();
    }

    score
}

fn format_context_chunk_for_verification(chunk: &LLMContextChunk) -> String {
    let mut header = format!(
        "{:?} - {}{}",
        chunk.context_type,
        chunk.file_path.display(),
        chunk
            .line_range
            .map(|(start, end)| format!(":{}-{}", start, end))
            .unwrap_or_default()
    );

    if let Some(provenance) = chunk.provenance.as_ref() {
        header.push_str(" | ");
        header.push_str(&provenance.to_string());
    }

    format!("{}\n{}", header, chunk.content)
}

fn diff_snippet_for_comment(diff: &UnifiedDiff, line_number: usize) -> String {
    for hunk in &diff.hunks {
        let hunk_start = hunk.new_start;
        let hunk_end = hunk.new_start + hunk.new_lines.saturating_sub(1);
        if (hunk_start..=hunk_end.max(hunk_start)).contains(&line_number) {
            return hunk
                .changes
                .iter()
                .map(|change| {
                    let prefix = match change.change_type {
                        crate::core::diff_parser::ChangeType::Added => "+",
                        crate::core::diff_parser::ChangeType::Removed => "-",
                        crate::core::diff_parser::ChangeType::Context => " ",
                    };
                    format!("{}{}", prefix, change.content)
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}

fn source_context_for_line(content: &str, line_number: usize, radius: usize) -> String {
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
