use std::path::{Path, PathBuf};

use crate::core;

pub(super) fn build_related_test_chunks(
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    find_test_files(file_path, repo_path)
        .iter()
        .take(2)
        .filter_map(|test_path| build_test_chunk(test_path, repo_path))
        .collect()
}

fn build_test_chunk(test_path: &Path, repo_path: &Path) -> Option<core::LLMContextChunk> {
    let relative = test_path.strip_prefix(repo_path).unwrap_or(test_path);
    let content = std::fs::read_to_string(test_path).ok()?;
    let snippet = content.lines().take(60).collect::<Vec<_>>().join("\n");
    if snippet.is_empty() {
        return None;
    }

    Some(
        core::LLMContextChunk::reference(relative.to_path_buf(), format!("[Test file]\n{snippet}"))
            .with_provenance(core::ContextProvenance::RelatedTestFile),
    )
}

fn find_test_files(file_path: &Path, repo_path: &Path) -> Vec<PathBuf> {
    let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = file_path.parent().unwrap_or(Path::new(""));

    vec![
        repo_path.join(parent).join(format!("test_{stem}.{ext}")),
        repo_path.join(parent).join(format!("{stem}_test.{ext}")),
        repo_path.join(parent).join(format!("{stem}.test.{ext}")),
        repo_path.join(parent).join(format!("{stem}.spec.{ext}")),
        repo_path
            .join(parent)
            .join("tests")
            .join(format!("{stem}.{ext}")),
        repo_path
            .join(parent)
            .join("__tests__")
            .join(format!("{stem}.{ext}")),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .collect()
}
