use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::config;
use crate::core;

pub(super) fn chunk_diff_for_context(diff_content: &str, max_chars: usize) -> Vec<String> {
    if diff_content.len() <= max_chars {
        return vec![diff_content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for section in diff_content.split("\ndiff --git ") {
        let section = if chunks.is_empty() && current_chunk.is_empty() {
            section.to_string()
        } else {
            format!("diff --git {}", section)
        };

        if current_chunk.len() + section.len() > max_chars && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = section;
        } else {
            current_chunk.push_str(&section);
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

pub(super) fn detect_instruction_files(repo_path: &Path) -> Vec<(String, String)> {
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

pub(super) fn gather_git_log(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--numstat",
            "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ai%n%n    %s%n",
            "-100",
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let log_text = String::from_utf8_lossy(&out.stdout).to_string();
            if log_text.trim().is_empty() {
                None
            } else {
                info!("Gathered git log ({} bytes)", log_text.len());
                Some(log_text)
            }
        }
        _ => {
            info!("Git log unavailable (not a git repo or git not found)");
            None
        }
    }
}

pub(super) fn resolve_convention_store_path(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|dir| dir.join("diffscope").join("conventions.json"))
}

pub(super) fn save_convention_store(
    store: &core::convention_learner::ConventionStore,
    path: &PathBuf,
) {
    if let Ok(json) = store.to_json() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(path, json) {
            warn!(
                "Failed to save convention store to {}: {}",
                path.display(),
                error
            );
        }
    }
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

    #[test]
    fn chunk_diff_small_diff_returns_single_chunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+hello\n";
        let chunks = chunk_diff_for_context(diff, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], diff);
    }

    #[test]
    fn chunk_diff_splits_at_file_boundaries() {
        let diff = "diff --git a/a.rs b/a.rs\n+line1\n\ndiff --git a/b.rs b/b.rs\n+line2\n\ndiff --git a/c.rs b/c.rs\n+line3\n";
        let chunks = chunk_diff_for_context(diff, 40);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.contains("diff --git"));
        }
    }

    #[test]
    fn chunk_diff_empty_input() {
        let chunks = chunk_diff_for_context("", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn chunk_diff_single_large_file_not_split_midfile() {
        let diff = format!("diff --git a/big.rs b/big.rs\n{}", "+line\n".repeat(100));
        let chunks = chunk_diff_for_context(&diff, 50);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_diff_preserves_all_content() {
        let file_a = "diff --git a/a.rs b/a.rs\n+alpha\n";
        let file_b = "\ndiff --git a/b.rs b/b.rs\n+beta\n";
        let file_c = "\ndiff --git a/c.rs b/c.rs\n+gamma\n";
        let diff = format!("{}{}{}", file_a, file_b, file_c);
        let chunks = chunk_diff_for_context(&diff, 50);
        let rejoined = chunks.join("");
        assert!(rejoined.contains("+alpha"));
        assert!(rejoined.contains("+beta"));
        assert!(rejoined.contains("+gamma"));
    }
}
