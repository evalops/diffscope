use std::path::{Path, PathBuf};

use crate::core;

pub(in crate::review::pipeline) fn gather_related_file_context(
    index: &core::SymbolIndex,
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    let mut chunks: Vec<core::LLMContextChunk> = Vec::new();

    let callers = index.reverse_deps(file_path);
    for caller_path in callers.iter().take(3) {
        if let Some(summary) = index.file_summary(caller_path) {
            let truncated: String = if summary.len() > 2000 {
                let mut end = 2000;
                while end > 0 && !summary.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &summary[..end])
            } else {
                summary.to_string()
            };
            chunks.push(
                core::LLMContextChunk::reference(
                    caller_path.clone(),
                    format!("[Caller/dependent file]\n{}", truncated),
                )
                .with_provenance(core::ContextProvenance::ReverseDependencySummary),
            );
        }
    }

    let test_files = find_test_files(file_path, repo_path);
    for test_path in test_files.iter().take(2) {
        let relative: &Path = test_path.strip_prefix(repo_path).unwrap_or(test_path);
        if let Ok(content) = std::fs::read_to_string(test_path) {
            let snippet: String = content.lines().take(60).collect::<Vec<_>>().join("\n");
            if !snippet.is_empty() {
                chunks.push(
                    core::LLMContextChunk::reference(
                        relative.to_path_buf(),
                        format!("[Test file]\n{}", snippet),
                    )
                    .with_provenance(core::ContextProvenance::RelatedTestFile),
                );
            }
        }
    }

    chunks
}

fn find_test_files(file_path: &Path, repo_path: &Path) -> Vec<PathBuf> {
    let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = file_path.parent().unwrap_or(Path::new(""));

    let candidates: Vec<PathBuf> = vec![
        repo_path
            .join(parent)
            .join(format!("test_{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}_test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.spec.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("tests")
            .join(format!("{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("__tests__")
            .join(format!("{}.{}", stem, ext)),
    ];

    candidates
        .into_iter()
        .filter(|path: &PathBuf| path.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_utf8_safe_truncation() {
        let prefix = "a".repeat(1999);
        let value = format!("{}€rest", prefix);
        assert!(value.len() > 2000);

        let mut end = 2000;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &value[..end];

        assert_eq!(end, 1999);
        assert!(truncated.starts_with("aaa"));
        assert!(!truncated.contains('€'));
    }
}
