use std::path::Path;
use tracing::{info, warn};

pub(in super::super) fn detect_instruction_files(repo_path: &Path) -> Vec<(String, String)> {
    const INSTRUCTION_FILES: &[&str] = &[
        ".cursorrules",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "agents.md",
        ".github/copilot-instructions.md",
        "GEMINI.md",
        ".diffscope-instructions.md",
    ];
    const MAX_INSTRUCTION_SIZE: u64 = 10_000;

    let mut results = Vec::new();
    for filename in INSTRUCTION_FILES {
        let path = repo_path.join(filename);
        if path.is_file() {
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > MAX_INSTRUCTION_SIZE {
                    warn!(
                        "Skipping instruction file {} ({} bytes exceeds {})",
                        filename,
                        meta.len(),
                        MAX_INSTRUCTION_SIZE
                    );
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    info!("Auto-detected instruction file: {}", filename);
                    results.push((filename.to_string(), trimmed));
                }
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_instruction_files_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = detect_instruction_files(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn detect_instruction_files_finds_cursorrules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".cursorrules"), "Use tabs not spaces").unwrap();
        let results = detect_instruction_files(dir.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, ".cursorrules");
        assert!(results[0].1.contains("Use tabs"));
    }
}
