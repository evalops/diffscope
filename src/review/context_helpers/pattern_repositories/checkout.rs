use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use tracing::warn;

use super::git::is_safe_git_url;

pub(super) fn prepare_pattern_repository_checkout(source: &str) -> Option<PathBuf> {
    use std::process::Command;

    if !is_safe_git_url(source) {
        warn!(
            "Rejecting pattern repository '{}': only https://, ssh://, and git@ URLs are allowed",
            source
        );
        return None;
    }

    let repo_dir = pattern_repository_cache_dir(source)?;
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

fn pattern_repository_cache_dir(source: &str) -> Option<PathBuf> {
    let home_dir = dirs::home_dir()?;
    let cache_root = home_dir.join(".diffscope").join("pattern_repositories");
    std::fs::create_dir_all(&cache_root).ok()?;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    Some(cache_root.join(format!("{:x}", hasher.finish())))
}
