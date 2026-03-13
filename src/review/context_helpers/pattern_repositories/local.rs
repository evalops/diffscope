use std::path::{Path, PathBuf};

pub(super) fn resolve_local_repository_path(source: &str, repo_root: &Path) -> Option<PathBuf> {
    let source_path = Path::new(source);
    if source_path.is_absolute() && source_path.is_dir() {
        return source_path.canonicalize().ok();
    }

    let repo_relative = repo_root.join(source);
    if repo_relative.is_dir() {
        return repo_relative.canonicalize().ok();
    }

    None
}
