use std::path::{Path, PathBuf};

use super::super::super::super::EvalFixture;

pub(super) fn resolve_repo_path(fixture_dir: &Path, fixture: &EvalFixture) -> PathBuf {
    fixture
        .repo_path
        .clone()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                fixture_dir.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("."))
}
