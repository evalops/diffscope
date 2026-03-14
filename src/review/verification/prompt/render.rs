use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::{Comment, LLMContextChunk, UnifiedDiff};

use super::evidence::{diff_snippet_for_comment, source_context_for_line};
use super::support::supporting_context_for_comment;

pub(super) fn render_comment_section(
    index: usize,
    comment: &Comment,
    diff: Option<&UnifiedDiff>,
    source_files: &HashMap<PathBuf, String>,
    extra_context: &HashMap<PathBuf, Vec<LLMContextChunk>>,
) -> String {
    let mut section = format!(
        "### Finding {}\n- File: {}:{}\n- Issue: {}\n",
        index + 1,
        comment.file_path.display(),
        comment.line_number,
        comment.content,
    );

    if let Some(suggestion) = comment.suggestion.as_ref() {
        section.push_str(&format!("- Suggestion: {suggestion}\n"));
    }

    if let Some(diff) = diff {
        let diff_snippet = diff_snippet_for_comment(diff, comment.line_number);
        append_code_block(&mut section, "- Diff evidence:\n", "diff", &diff_snippet);
    }

    if let Some(content) = source_files.get(&comment.file_path) {
        let file_context = source_context_for_line(content, comment.line_number, 6);
        append_code_block(&mut section, "- Nearby file context:\n", "", &file_context);
    }

    let supporting_context = supporting_context_for_comment(comment, extra_context);
    if !supporting_context.is_empty() {
        section.push_str("- Cross-file attachment rule: if this changed line introduces a risky call or tainted input into the helper below, the finding can still be accurate and line-correct even when the vulnerable sink lives in the supporting-context file.\n");
        section.push_str("- Supporting context:\n");
        for chunk in supporting_context {
            section.push_str("```text\n");
            section.push_str(&format_context_chunk_for_verification(&chunk));
            section.push_str("\n```\n");
        }
    }

    section.push('\n');
    section
}

fn append_code_block(section: &mut String, label: &str, language: &str, content: &str) {
    if content.trim().is_empty() {
        return;
    }

    section.push_str(label);
    section.push_str(&format!("```{language}\n"));
    section.push_str(content);
    section.push_str("\n```\n");
}

fn format_context_chunk_for_verification(chunk: &LLMContextChunk) -> String {
    let mut header = format!(
        "{:?} - {}{}",
        chunk.context_type,
        chunk.file_path.display(),
        chunk
            .line_range
            .map(|(start, end)| format!(":{start}-{end}"))
            .unwrap_or_default()
    );

    if let Some(provenance) = chunk.provenance.as_ref() {
        header.push_str(" | ");
        header.push_str(&provenance.to_string());
    }

    format!("{}\n{}", header, chunk.content)
}
