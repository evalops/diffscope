use std::path::Path;

use crate::core;

pub(super) fn build_caller_context_chunks(
    index: &core::SymbolIndex,
    file_path: &Path,
) -> Vec<core::LLMContextChunk> {
    index
        .reverse_deps(file_path)
        .iter()
        .take(3)
        .filter_map(|caller_path| {
            let summary = index.file_summary(caller_path)?;
            Some(
                core::LLMContextChunk::reference(
                    caller_path.clone(),
                    format!("[Caller/dependent file]\n{}", truncate_summary(summary)),
                )
                .with_provenance(core::ContextProvenance::ReverseDependencySummary),
            )
        })
        .collect()
}

fn truncate_summary(summary: &str) -> String {
    if summary.len() <= 2000 {
        return summary.to_string();
    }

    let mut end = 2000;
    while end > 0 && !summary.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &summary[..end])
}

#[cfg(test)]
mod tests {
    use super::truncate_summary;

    #[test]
    fn test_utf8_safe_truncation() {
        let prefix = "a".repeat(1999);
        let value = format!("{}€rest", prefix);
        assert!(value.len() > 2000);

        let truncated = truncate_summary(&value);
        assert!(truncated.starts_with("aaa"));
        assert!(!truncated.contains('€'));
        assert!(truncated.ends_with("...[truncated]"));
    }
}
