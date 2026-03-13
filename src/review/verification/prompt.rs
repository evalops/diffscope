use std::collections::HashMap;

use crate::core::{Comment, LLMContextChunk, UnifiedDiff};

#[path = "prompt/evidence.rs"]
mod evidence;
#[path = "prompt/render.rs"]
mod render;
#[path = "prompt/support.rs"]
mod support;

use render::render_comment_section;

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
    for (index, comment) in comments.iter().enumerate() {
        prompt.push_str(&render_comment_section(
            index,
            comment,
            diff_map.get(&comment.file_path).copied(),
            source_files,
            extra_context,
        ));
    }

    prompt.push_str("Return JSON only. Do not add commentary outside the JSON array.\n");
    prompt
}
