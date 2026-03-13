use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::config;

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

/// Check whether a source string looks like a git URL and uses an allowed scheme.
/// Accepts `https://`, `ssh://`, `git@` (SSH shorthand), and `http://` (with `.git` suffix).
fn is_safe_git_url(source: &str) -> bool {
    source.starts_with("https://")
        || source.starts_with("ssh://")
        || source.starts_with("git@")
        || (source.starts_with("http://") && source.ends_with(".git"))
}

fn is_git_source(source: &str) -> bool {
    is_safe_git_url(source)
}

fn prepare_pattern_repository_checkout(source: &str) -> Option<PathBuf> {
    use std::process::Command;

    if !is_safe_git_url(source) {
        warn!(
            "Rejecting pattern repository '{}': only https://, ssh://, and git@ URLs are allowed",
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn test_is_git_source_accepts_ssh() {
        assert!(is_git_source("ssh://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_allows_https() {
        assert!(is_safe_git_url("https://github.com/org/repo"));
        assert!(is_safe_git_url("https://gitlab.com/org/repo.git"));
    }

    #[test]
    fn test_is_safe_git_url_allows_ssh() {
        assert!(is_safe_git_url("git@github.com:org/repo.git"));
        assert!(is_safe_git_url("ssh://example.com/repo"));
        assert!(is_safe_git_url("ssh://git@gitlab.internal/org/rules.git"));
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
        assert!(!is_safe_git_url("gopher://example.com/repo"));
    }

    #[test]
    fn test_is_safe_git_url_rejects_http_without_git_suffix() {
        assert!(!is_safe_git_url("http://example.com/repo"));
    }
}
