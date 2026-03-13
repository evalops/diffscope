use std::path::Path;

use crate::core;

use super::callers::build_caller_context_chunks;
use super::test_files::build_related_test_chunks;

pub(in crate::review::pipeline) fn gather_related_file_context(
    index: &core::SymbolIndex,
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    let mut chunks = build_caller_context_chunks(index, file_path);
    chunks.extend(build_related_test_chunks(file_path, repo_path));
    chunks
}
