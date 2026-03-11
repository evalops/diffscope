use anyhow::Result;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::config;
use crate::core;

pub type PatternRepositoryMap = HashMap<String, PathBuf>;

pub fn resolve_pattern_repositories(
    config: &config::Config,
    repo_root: &Path,
) -> PatternRepositoryMap {
    let mut resolved = HashMap::new();
    if config.pattern_repositories.is_empty() {
        return resolved;
    }

    for repo in &config.pattern_repositories {
        if resolved.contains_key(&repo.source) {
            continue;
        }

        let source_path = Path::new(&repo.source);
        if source_path.is_absolute() && source_path.is_dir() {
            if let Ok(path) = source_path.canonicalize() {
                resolved.insert(repo.source.clone(), path);
            }
            continue;
        }

        let repo_relative = repo_root.join(&repo.source);
        if repo_relative.is_dir() {
            if let Ok(path) = repo_relative.canonicalize() {
                resolved.insert(repo.source.clone(), path);
            }
            continue;
        }

        if is_git_source(&repo.source) {
            if let Some(path) = prepare_pattern_repository_checkout(&repo.source) {
                resolved.insert(repo.source.clone(), path);
                continue;
            }
        }

        warn!(
            "Skipping pattern repository '{}' (not a readable local path or cloneable git source)",
            repo.source
        );
    }

    resolved
}

fn is_git_source(source: &str) -> bool {
    if source.starts_with("https://") || source.starts_with("git@") {
        return true;
    }
    if source.starts_with("http://") && source.ends_with(".git") {
        return true;
    }
    false
}

/// Validate that a pattern repository source URL uses an allowed scheme.
/// Only `https://`, `git@`, and `http://` (with `.git` suffix) are permitted.
fn is_safe_git_url(source: &str) -> bool {
    source.starts_with("https://")
        || source.starts_with("git@")
        || (source.starts_with("http://") && source.ends_with(".git"))
}

fn prepare_pattern_repository_checkout(source: &str) -> Option<PathBuf> {
    use std::process::Command;

    if !is_safe_git_url(source) {
        warn!(
            "Rejecting pattern repository '{}': only https:// and git@ URLs are allowed",
            source
        );
        return None;
    }

    let home_dir = dirs::home_dir()?;
    let cache_root = home_dir.join(".diffscope").join("pattern_repositories");
    if std::fs::create_dir_all(&cache_root).is_err() {
        return None;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    let repo_dir = cache_root.join(format!("{:x}", hasher.finish()));

    if repo_dir.is_dir() {
        let pull_result = Command::new("git")
            .arg("-C")
            .arg(&repo_dir)
            .arg("pull")
            .arg("--ff-only")
            .output();
        if let Err(err) = pull_result {
            warn!(
                "Unable to update cached pattern repository {}: {}",
                source, err
            );
        }
    } else {
        let clone_result = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(source)
            .arg(&repo_dir)
            .output();
        match clone_result {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    "Failed to clone pattern repository {}: {}",
                    source,
                    stderr.trim()
                );
                return None;
            }
            Err(err) => {
                warn!("Failed to clone pattern repository {}: {}", source, err);
                return None;
            }
        }
    }

    if repo_dir.is_dir() {
        Some(repo_dir)
    } else {
        None
    }
}

pub async fn inject_custom_context(
    config: &config::Config,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    for entry in config.matching_custom_context(&diff.file_path) {
        if !entry.notes.is_empty() {
            context_chunks.push(core::LLMContextChunk {
                content: format!("Custom context notes:\n{}", entry.notes.join("\n")),
                context_type: core::ContextType::Documentation,
                file_path: diff.file_path.clone(),
                line_range: None,
            });
        }

        if !entry.files.is_empty() {
            let extra_chunks = context_fetcher
                .fetch_additional_context(&entry.files)
                .await?;
            context_chunks.extend(extra_chunks);
        }
    }

    Ok(())
}

pub async fn inject_pattern_repository_context(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let mut sources_seen = HashSet::new();
    for repo in config.matching_pattern_repositories(&diff.file_path) {
        if !sources_seen.insert(repo.source.clone()) {
            continue;
        }

        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let mut chunks = context_fetcher
            .fetch_additional_context_from_base(
                base_path,
                &repo.include_patterns,
                repo.max_files,
                repo.max_lines,
            )
            .await?;

        if chunks.is_empty() {
            continue;
        }

        context_chunks.push(core::LLMContextChunk {
            content: format!("Pattern repository context source: {}", repo.source),
            context_type: core::ContextType::Documentation,
            file_path: diff.file_path.clone(),
            line_range: None,
        });

        for chunk in &mut chunks {
            chunk.content = format!("[Pattern repository: {}]\n{}", repo.source, chunk.content);
        }
        context_chunks.extend(chunks);
    }

    Ok(())
}

pub fn rank_and_trim_context_chunks(
    diff: &core::UnifiedDiff,
    chunks: Vec<core::LLMContextChunk>,
    max_chunks: usize,
    max_chars: usize,
) -> Vec<core::LLMContextChunk> {
    if chunks.is_empty() {
        return chunks;
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for chunk in chunks {
        let key = format!(
            "{}|{:?}|{:?}|{}",
            chunk.file_path.display(),
            chunk.context_type,
            chunk.line_range,
            chunk.content
        );
        if seen.insert(key) {
            deduped.push(chunk);
        }
    }

    let changed_ranges: Vec<(usize, usize)> = diff
        .hunks
        .iter()
        .map(|hunk| {
            (
                hunk.new_start.max(1),
                hunk.new_start
                    .saturating_add(hunk.new_lines.saturating_sub(1))
                    .max(hunk.new_start.max(1)),
            )
        })
        .collect();

    let mut scored: Vec<(i32, usize, core::LLMContextChunk)> = deduped
        .into_iter()
        .map(|chunk| {
            let mut score = match chunk.context_type {
                core::ContextType::FileContent => 130,
                core::ContextType::Definition => 100,
                core::ContextType::Reference => 80,
                core::ContextType::Documentation => 60,
            };

            if chunk.file_path == diff.file_path {
                score += 90;
            }

            if let Some(range) = chunk.line_range {
                if changed_ranges
                    .iter()
                    .any(|candidate| ranges_overlap(*candidate, range))
                {
                    score += 70;
                } else if chunk.file_path == diff.file_path {
                    score += 20;
                }
            }

            if chunk.content.starts_with("Active review rules.") {
                score += 120;
            } else if chunk
                .content
                .starts_with("Pattern repository context source:")
            {
                score += 30;
            } else if chunk.content.starts_with("[Pattern repository:") {
                score += 25;
            }

            if chunk.content.len() > 4000 {
                score -= 10;
            }

            (score, chunk.content.len(), chunk)
        })
        .collect();

    scored.sort_by_key(|(score, len, _)| (Reverse(*score), *len));

    let max_chunks = if max_chunks == 0 {
        usize::MAX
    } else {
        max_chunks
    };
    let max_chars = if max_chars == 0 {
        usize::MAX
    } else {
        max_chars
    };

    let mut kept = Vec::new();
    let mut used_chars = 0usize;

    for (_, _, chunk) in scored {
        if kept.len() >= max_chunks {
            break;
        }

        let chunk_len = chunk.content.len();
        if used_chars.saturating_add(chunk_len) > max_chars {
            continue;
        }

        used_chars = used_chars.saturating_add(chunk_len);
        kept.push(chunk);
    }

    if kept.is_empty() {
        return Vec::new();
    }

    kept
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_overlap_true() {
        assert!(ranges_overlap((1, 10), (5, 15)));
        assert!(ranges_overlap((5, 15), (1, 10)));
        assert!(ranges_overlap((1, 10), (1, 10)));
        assert!(ranges_overlap((1, 10), (10, 20)));
    }

    #[test]
    fn ranges_overlap_false() {
        assert!(!ranges_overlap((1, 5), (6, 10)));
        assert!(!ranges_overlap((10, 20), (1, 5)));
    }

    #[test]
    fn rank_and_trim_empty_chunks() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let result = rank_and_trim_context_chunks(&diff, vec![], 10, 10000);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_and_trim_deduplicates() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunk = core::LLMContextChunk {
            content: "duplicate content".to_string(),
            context_type: core::ContextType::Documentation,
            file_path: PathBuf::from("test.rs"),
            line_range: None,
        };
        let chunks = vec![chunk.clone(), chunk];
        let result = rank_and_trim_context_chunks(&diff, chunks, 10, 100000);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn rank_and_trim_respects_max_chunks() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks: Vec<core::LLMContextChunk> = (0..5)
            .map(|i| core::LLMContextChunk {
                content: format!("chunk {}", i),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
            })
            .collect();
        let result = rank_and_trim_context_chunks(&diff, chunks, 2, 100000);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn rank_and_trim_respects_max_chars() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks: Vec<core::LLMContextChunk> = (0..5)
            .map(|i| core::LLMContextChunk {
                content: format!("chunk {} with some content here", i),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
            })
            .collect();
        // Each chunk is ~30 chars, setting max to 60 should keep at most 2
        let result = rank_and_trim_context_chunks(&diff, chunks, 100, 60);
        assert!(result.len() <= 2);
    }

    #[test]
    fn rank_and_trim_prioritizes_same_file() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("target.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks = vec![
            core::LLMContextChunk {
                content: "other file content".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("other.rs"),
                line_range: None,
            },
            core::LLMContextChunk {
                content: "target file content".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("target.rs"),
                line_range: None,
            },
        ];
        let result = rank_and_trim_context_chunks(&diff, chunks, 1, 100000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, PathBuf::from("target.rs"));
    }

    #[test]
    fn rank_and_trim_rule_chunks_ranked_high() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks = vec![
            core::LLMContextChunk {
                content: "some reference".to_string(),
                context_type: core::ContextType::Reference,
                file_path: PathBuf::from("other.rs"),
                line_range: None,
            },
            core::LLMContextChunk {
                content: "Active review rules. Check these rules.".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
            },
        ];
        let result = rank_and_trim_context_chunks(&diff, chunks, 1, 100000);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.starts_with("Active review rules."));
    }

    // === URL validation tests ===

    #[test]
    fn test_is_git_source_https() {
        assert!(is_git_source("https://github.com/org/repo.git"));
        assert!(is_git_source("https://github.com/org/repo"));
    }

    #[test]
    fn test_is_git_source_ssh() {
        assert!(is_git_source("git@github.com:org/repo.git"));
    }

    #[test]
    fn test_is_git_source_http_with_git_suffix() {
        assert!(is_git_source("http://example.com/repo.git"));
    }

    #[test]
    fn test_is_git_source_rejects_local_paths() {
        assert!(!is_git_source("/tmp/evil"));
        assert!(!is_git_source("../relative/path"));
        assert!(!is_git_source("file:///etc/passwd"));
    }

    #[test]
    fn test_is_git_source_rejects_other_schemes() {
        assert!(!is_git_source("ftp://example.com/repo.git"));
        assert!(!is_git_source("ssh://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_allows_https() {
        assert!(is_safe_git_url("https://github.com/org/repo"));
        assert!(is_safe_git_url("https://gitlab.com/org/repo.git"));
    }

    #[test]
    fn test_is_safe_git_url_allows_ssh() {
        assert!(is_safe_git_url("git@github.com:org/repo.git"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_file_urls() {
        assert!(!is_safe_git_url("file:///etc/passwd"));
        assert!(!is_safe_git_url("/tmp/evil"));
        assert!(!is_safe_git_url("../traversal"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_arbitrary_schemes() {
        assert!(!is_safe_git_url("ftp://example.com/repo"));
        assert!(!is_safe_git_url("ssh://example.com/repo"));
        assert!(!is_safe_git_url("gopher://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_http_without_git_suffix() {
        // Plain http without .git suffix is not safe enough
        assert!(!is_safe_git_url("http://example.com/repo"));
    }
}
