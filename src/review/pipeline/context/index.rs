use anyhow::Result;
use git2::{Repository, Status, StatusOptions};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::config;
use crate::core;

pub fn build_symbol_index(config: &config::Config, repo_root: &Path) -> Option<core::SymbolIndex> {
    build_symbol_index_with_cache_path(config, repo_root, None)
}

fn build_symbol_index_with_cache_path(
    config: &config::Config,
    repo_root: &Path,
    cache_path_override: Option<&Path>,
) -> Option<core::SymbolIndex> {
    if !config.symbol_index {
        return None;
    }

    let provider = config.symbol_index_provider.as_str();
    let detected_command = if provider == "lsp" && config.symbol_index_lsp_command.is_none() {
        core::SymbolIndex::detect_lsp_command(
            repo_root,
            config.symbol_index_max_files,
            &config.symbol_index_lsp_languages,
            |path| config.should_exclude(path),
        )
    } else {
        None
    };

    let command = config
        .symbol_index_lsp_command
        .as_deref()
        .map(str::to_string)
        .or(detected_command);

    if provider == "lsp" && config.symbol_index_lsp_command.is_none() {
        if let Some(command) = command.as_deref() {
            info!("Auto-detected LSP command: {}", command);
        }
    }

    let cache_path =
        symbol_index_cache_path(config, repo_root, command.as_deref(), cache_path_override);

    if let Some(cache_path) = cache_path.as_deref() {
        if let Some(index) = core::load_symbol_index(cache_path) {
            info!(
                "Loaded persisted repository graph from {}",
                cache_path.display()
            );
            return Some(index);
        }
    }

    let result = build_symbol_index_inner(config, repo_root, command.as_deref());

    match result {
        Ok((index, cacheable)) => {
            if cacheable {
                if let Some(cache_path) = cache_path.as_deref() {
                    if let Err(err) = core::save_symbol_index(cache_path, &index) {
                        warn!(
                            "Failed to persist repository graph at {}: {}",
                            cache_path.display(),
                            err
                        );
                    }
                }
            }
            info!(
                "Indexed {} symbols across {} files",
                index.symbols_indexed(),
                index.files_indexed()
            );
            Some(index)
        }
        Err(err) => {
            warn!("Symbol index build failed: {}", err);
            None
        }
    }
}

fn build_symbol_index_inner(
    config: &config::Config,
    repo_root: &Path,
    command: Option<&str>,
) -> Result<(core::SymbolIndex, bool)> {
    if config.symbol_index_provider == "lsp" {
        if let Some(command) = command {
            match core::SymbolIndex::build_with_lsp(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                command,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            ) {
                Ok(index) => Ok((index, true)),
                Err(err) => {
                    warn!("LSP indexer failed (falling back to regex): {}", err);
                    Ok((
                        core::SymbolIndex::build(
                            repo_root,
                            config.symbol_index_max_files,
                            config.symbol_index_max_bytes,
                            config.symbol_index_max_locations,
                            |path| config.should_exclude(path),
                        )?,
                        false,
                    ))
                }
            }
        } else {
            warn!("No LSP command configured or detected; falling back to regex indexer.");
            Ok((
                core::SymbolIndex::build(
                    repo_root,
                    config.symbol_index_max_files,
                    config.symbol_index_max_bytes,
                    config.symbol_index_max_locations,
                    |path| config.should_exclude(path),
                )?,
                true,
            ))
        }
    } else {
        Ok((
            core::SymbolIndex::build(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                |path| config.should_exclude(path),
            )?,
            true,
        ))
    }
}

fn symbol_index_cache_path(
    config: &config::Config,
    repo_root: &Path,
    command: Option<&str>,
    cache_path_override: Option<&Path>,
) -> Option<PathBuf> {
    let revision = clean_repo_revision(repo_root)?;
    let cache_descriptor = json!({
        "revision": revision,
        "provider": config.symbol_index_provider,
        "command": command,
        "max_files": config.symbol_index_max_files,
        "max_bytes": config.symbol_index_max_bytes,
        "max_locations": config.symbol_index_max_locations,
        "lsp_languages": normalized_lsp_languages(config),
        "exclude_patterns": config.exclude_patterns,
        "path_ignore_patterns": path_ignore_patterns(config),
    });
    let cache_key = hash_cache_descriptor(&cache_descriptor.to_string());

    Some(
        cache_path_override
            .map(Path::to_path_buf)
            .unwrap_or_else(|| core::default_symbol_index_path(repo_root, &cache_key)),
    )
}

fn clean_repo_revision(repo_root: &Path) -> Option<String> {
    let repo = Repository::discover(repo_root).ok()?;
    if repo_has_uncommitted_changes(&repo) {
        info!("Skipping persisted repository graph cache because the repository is dirty.");
        return None;
    }

    let revision = repo.head().ok()?.target().map(|oid| oid.to_string());
    revision
}

fn repo_has_uncommitted_changes(repo: &Repository) -> bool {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    repo.statuses(Some(&mut options))
        .map(|statuses| {
            statuses
                .iter()
                .any(|entry| entry.status() != Status::CURRENT)
        })
        .unwrap_or(true)
}

fn normalized_lsp_languages(config: &config::Config) -> Vec<(String, String)> {
    let mut languages = config
        .symbol_index_lsp_languages
        .iter()
        .map(|(extension, language)| (extension.clone(), language.clone()))
        .collect::<Vec<_>>();
    languages.sort_by(|left, right| left.0.cmp(&right.0));
    languages
}

fn path_ignore_patterns(config: &config::Config) -> Vec<(String, Vec<String>)> {
    let mut entries = config
        .paths
        .iter()
        .filter(|(_, path_config)| !path_config.ignore_patterns.is_empty())
        .map(|(path, path_config)| (path.clone(), path_config.ignore_patterns.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn hash_cache_descriptor(descriptor: &str) -> String {
    format!("{:x}", Sha256::digest(descriptor.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::build_symbol_index_with_cache_path;
    use crate::config;
    use git2::{Repository, Signature};
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        dir
    }

    fn commit_repo_file(repo_root: &Path, relative: &str, content: &str, message: &str) {
        let repo = Repository::open(repo_root).unwrap();
        let path = repo_root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new(relative)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        let parent_commit = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .and_then(|oid| repo.find_commit(oid).ok());

        if let Some(parent_commit) = parent_commit.as_ref() {
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[parent_commit],
            )
            .unwrap();
        } else {
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .unwrap();
        }
    }

    fn regex_config() -> config::Config {
        config::Config {
            symbol_index_provider: "regex".to_string(),
            symbol_index_max_files: 16,
            symbol_index_max_bytes: 128 * 1024,
            symbol_index_max_locations: 8,
            ..config::Config::default()
        }
    }

    #[test]
    fn build_symbol_index_reloads_persisted_graph_when_revision_is_clean() {
        let dir = init_git_repo();
        commit_repo_file(
            dir.path(),
            "src/lib.rs",
            "pub fn helper() {}\n",
            "add helper",
        );
        let config = regex_config();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("symbol-index.json");

        let first = build_symbol_index_with_cache_path(&config, dir.path(), Some(&cache_path))
            .expect("expected symbol index");
        let first_summary = first.graph_metadata_summary(dir.path());

        std::thread::sleep(Duration::from_secs(1));

        let second = build_symbol_index_with_cache_path(&config, dir.path(), Some(&cache_path))
            .expect("expected cached symbol index");
        let second_summary = second.graph_metadata_summary(dir.path());

        assert!(cache_path.exists());
        assert_eq!(first_summary, second_summary);
    }

    #[test]
    fn build_symbol_index_skips_cache_when_worktree_is_dirty() {
        let dir = init_git_repo();
        commit_repo_file(
            dir.path(),
            "src/lib.rs",
            "pub fn helper() {}\n",
            "add helper",
        );
        let config = regex_config();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("symbol-index.json");

        let initial = build_symbol_index_with_cache_path(&config, dir.path(), Some(&cache_path))
            .expect("expected initial symbol index");
        assert!(initial.lookup("changed").is_none());

        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn helper() {}\npub fn changed() {}\n",
        )
        .unwrap();

        let rebuilt = build_symbol_index_with_cache_path(&config, dir.path(), Some(&cache_path))
            .expect("expected rebuilt symbol index");

        assert!(rebuilt.lookup("changed").is_some());
    }
}
