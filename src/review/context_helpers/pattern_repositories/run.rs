use std::path::{Path, PathBuf};

use tracing::warn;

use crate::config;

use super::checkout::prepare_pattern_repository_checkout;
use super::git::is_git_source;
use super::local::resolve_local_repository_path;
use super::PatternRepositoryMap;

pub fn resolve_pattern_repositories(
    config: &config::Config,
    repo_root: &Path,
) -> PatternRepositoryMap {
    resolve_pattern_repositories_with(config, repo_root, prepare_pattern_repository_checkout)
}

pub(super) fn resolve_pattern_repositories_with<F>(
    config: &config::Config,
    repo_root: &Path,
    mut prepare_checkout: F,
) -> PatternRepositoryMap
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    let mut resolved = PatternRepositoryMap::new();
    if config.pattern_repositories.is_empty() {
        return resolved;
    }

    for repo in &config.pattern_repositories {
        if resolved.contains_key(&repo.source) {
            continue;
        }

        if let Some(path) = resolve_local_repository_path(&repo.source, repo_root) {
            resolved.insert(repo.source.clone(), path);
            continue;
        }

        if is_git_source(&repo.source) {
            if let Some(path) = prepare_checkout(&repo.source) {
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
